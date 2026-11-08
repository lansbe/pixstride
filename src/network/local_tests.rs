use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pixstride-local-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(bytes).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn read_limit_is_applied_to_the_actual_read_snapshot() {
    let fixture = Fixture::new(b"1234");
    let file = File::open(&fixture.0).unwrap();
    // A previous size check does not authorize a larger later snapshot.
    assert_eq!(file.metadata().unwrap().len(), 4);
    OpenOptions::new()
        .append(true)
        .open(&fixture.0)
        .unwrap()
        .write_all(b"5678")
        .unwrap();
    assert_eq!(
        read_checked_limit(file, 4, || {}).err().as_deref(),
        Some("source exceeds read limit")
    );
    assert_eq!(
        read_checked_limit(File::open(&fixture.0).unwrap(), 8, || {})
            .unwrap()
            .as_slice(),
        b"12345678"
    );
}

#[test]
fn changes_after_the_read_snapshot_are_rejected() {
    let fixture = Fixture::new(b"1234");
    let result = read_checked_limit(File::open(&fixture.0).unwrap(), 8, || {
        OpenOptions::new()
            .append(true)
            .open(&fixture.0)
            .unwrap()
            .write_all(b"5")
            .unwrap();
    });
    assert_eq!(
        result.err().as_deref(),
        Some("source file changed while reading")
    );
}
