use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn nested_paths_are_pinned_and_symlinks_never_escape() {
    let marker = Fixture::new(b"outside");
    let path = marker.0.with_extension("directory");
    std::fs::create_dir(&path).unwrap();
    std::fs::create_dir(path.join("nested")).unwrap();
    std::fs::write(path.join("nested/café image.ppm"), b"inside").unwrap();
    std::os::unix::fs::symlink(&marker.0, path.join("nested/link")).unwrap();
    std::os::unix::fs::symlink(path.join("nested"), path.join("alias")).unwrap();
    let root = LocalRoot::open(&path).unwrap();
    assert_eq!(
        root.read_with_modified("local:///nested/café image.ppm")
            .unwrap()
            .0
            .as_slice(),
        b"inside"
    );
    for name in [
        "../escape",
        "nested/../escape",
        "nested/link",
        "alias/café image.ppm",
        "nested//x",
        "/absolute",
    ] {
        assert!(
            root.read_with_modified(&format!("local:///{name}"))
                .is_err(),
            "{name}"
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

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
