//! Bounded native pipe protocol and per-request process resource budgets.
use super::{engine, timeout};
use crate::{request::Request, Error};
use std::io::{Read, Write};

#[cfg(target_os = "linux")]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(target_os = "linux"))]
type Resource = libc::c_int;

pub(super) const OUTPUT_LIMIT: usize = crate::MAX_FILE + 37;

pub(super) fn frame(input: &mut impl Read, limit: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut size = [0; 8];
    match input.read_exact(&mut size[..1]) {
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Ok(()) => {}
        Err(_) => return Err("native frame read failed"),
    }
    input
        .read_exact(&mut size[1..])
        .map_err(|_| "truncated native frame")?;
    let size = u64::from_le_bytes(size);
    if size > limit as u64 {
        return Err("native frame exceeds limit");
    }
    let mut data = vec![0; size as usize];
    input
        .read_exact(&mut data)
        .map_err(|_| "truncated native frame")?;
    Ok(Some(data))
}

fn required(input: &mut impl Read, limit: usize) -> Result<Vec<u8>, Error> {
    frame(input, limit)?.ok_or("truncated native request")
}

fn output(mut out: impl Write, image: engine::NativeResult, persistent: bool) -> Result<(), Error> {
    let engine::NativeResult {
        body,
        dimensions,
        origin,
        format,
    } = image;
    if body.len() > crate::MAX_FILE {
        return Err("native output exceeds limit");
    }
    if persistent {
        out.write_all(&((body.len() + 37) as u64).to_le_bytes())
            .map_err(|_| "native output write failed")?;
    }
    let id = match format {
        crate::request::OutputFormat::Png => 0,
        crate::request::OutputFormat::Jpeg => 1,
        crate::request::OutputFormat::Webp => 2,
        crate::request::OutputFormat::Gif => 3,
        crate::request::OutputFormat::Bmp => 4,
        crate::request::OutputFormat::Tiff => 5,
        crate::request::OutputFormat::Ico => 6,
        crate::request::OutputFormat::Avif => 7,
        crate::request::OutputFormat::Heic => 8,
        crate::request::OutputFormat::Jxl => 9,
        crate::request::OutputFormat::Svg => 10,
    };
    out.write_all(b"PXN2")
        .and_then(|_| out.write_all(&(dimensions[0] as u64).to_le_bytes()))
        .and_then(|_| out.write_all(&(dimensions[1] as u64).to_le_bytes()))
        .and_then(|_| out.write_all(&(origin[0] as u64).to_le_bytes()))
        .and_then(|_| out.write_all(&(origin[1] as u64).to_le_bytes()))
        .and_then(|_| out.write_all(&[id]))
        .and_then(|_| out.write_all(&body))
        .and_then(|_| out.flush())
        .map_err(|_| "native output write failed")
}

pub(super) fn run(persistent: bool) -> Result<(), Error> {
    limits(persistent)?;
    engine::initialize()?;
    let mut input = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    while let Some(options) = frame(&mut input, 65536)? {
        let request: Request =
            serde_json::from_slice(&options).map_err(|_| "invalid native request")?;
        let source = required(&mut input, crate::MAX_FILE)?;
        let mark = required(&mut input, 16 * 1024 * 1024)?;
        renew_cpu_budget(persistent)?;
        output(
            &mut out,
            engine::process(&source, &request, &mark)?,
            persistent,
        )?;
        if !persistent {
            break;
        }
    }
    Ok(())
}

fn set_limit(resource: Resource, value: u64) -> Result<(), Error> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: libc supplies the resource ABI; this only changes the dedicated child.
    if unsafe { libc::setrlimit(resource, &limit) } != 0 {
        return Err("cannot set native resource limit");
    }
    Ok(())
}

fn limits(persistent: bool) -> Result<(), Error> {
    set_limit(libc::RLIMIT_CORE, 0)?;
    set_limit(libc::RLIMIT_NOFILE, 128)?;
    #[cfg(target_os = "linux")]
    {
        set_limit(libc::RLIMIT_AS, 1024 * 1024 * 1024)?;
        if persistent {
            // A reused child cannot outlive a killed server. The second getppid
            // closes the race where its parent exits before prctl is installed.
            let parent = unsafe { libc::getppid() };
            if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0
                || unsafe { libc::getppid() } != parent
            {
                return Err("cannot bind native worker lifetime to parent");
            }
        }
    }
    renew_cpu_budget(persistent)
}

fn cpu_budget(seconds: u64, micros: u64, timeout: u64) -> u64 {
    seconds
        .saturating_add(micros.div_ceil(1_000_000))
        .saturating_add(timeout + 1)
}

fn renew_cpu_budget(persistent: bool) -> Result<(), Error> {
    let timeout = timeout()?;
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    // CPU limits are cumulative. Renew only the soft limit, preserving any
    // administrator-imposed hard ceiling, instead of granting an unbounded job.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0
        || unsafe { libc::getrlimit(libc::RLIMIT_CPU, &mut limit) } != 0
    {
        return Err("cannot read native CPU budget");
    }
    let value = cpu_budget(
        (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec).max(0) as u64,
        (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec).max(0) as u64,
        timeout,
    ) as libc::rlim_t;
    limit.rlim_cur = value.min(limit.rlim_max);
    if !persistent {
        limit.rlim_max = limit.rlim_cur;
    }
    if unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) } != 0 {
        return Err("cannot renew native CPU budget");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_are_separate_and_clean_eof_differs_from_truncation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u64.to_le_bytes());
        bytes.extend_from_slice(b"abc");
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.push(b'z');
        let mut input = bytes.as_slice();
        assert_eq!(frame(&mut input, 3).unwrap(), Some(b"abc".to_vec()));
        assert_eq!(frame(&mut input, 3).unwrap(), Some(b"z".to_vec()));
        assert_eq!(frame(&mut input, 3).unwrap(), None);
        assert!(frame(&mut &[1u8][..], 3).is_err());
        assert!(frame(&mut &[4u8, 0, 0, 0, 0, 0, 0, 0][..], 3).is_err());
        assert!(frame(&mut &[3u8, 0, 0, 0, 0, 0, 0, 0, b'a'][..], 3).is_err());
    }
    #[test]
    fn cpu_limit_renews_after_prior_images_and_rounds_partial_seconds_up() {
        assert_eq!(cpu_budget(0, 0, 1), 2);
        assert_eq!(cpu_budget(17, 1, 1), 20);
        assert_eq!(cpu_budget(17, 1_000_001, 10), 30);
    }
}
