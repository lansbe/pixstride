//! Discover native libraries only when the optional engine is enabled.
fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG");
    if std::env::var_os("CARGO_FEATURE_NATIVE_CODECS").is_none() {
        return;
    }
    let executable = std::env::var_os("PKG_CONFIG").unwrap_or_else(|| "pkg-config".into());
    let version = std::process::Command::new(&executable)
        .args(["--atleast-version=8.18", "vips"])
        .status()
        .expect("native-codecs requires pkg-config");
    assert!(
        version.success(),
        "native-codecs requires libvips 8.18 or newer"
    );
    let mut paths = std::collections::BTreeSet::new();
    for library in ["vips", "glib-2.0", "gobject-2.0"] {
        let output = std::process::Command::new(&executable)
            .args(["--variable=libdir", library])
            .output()
            .expect("native-codecs requires pkg-config and libvips development packages");
        assert!(
            output.status.success(),
            "cannot discover native library {library}"
        );
        let directory = String::from_utf8(output.stdout).expect("invalid native library directory");
        let directory = directory.trim();
        assert!(!directory.is_empty(), "missing native library {library}");
        println!("cargo:rustc-link-search=native={directory}");
        paths.insert(directory.to_owned());
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for directory in paths {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
        }
    }
}
