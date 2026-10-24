use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub fn fake_claude() -> String {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let profile = exe.parent().and_then(Path::parent).unwrap();
            let name = profile.file_name().and_then(|name| name.to_str()).unwrap();
            let status = Command::new(env!("CARGO"))
                .args(["build", "--quiet", "--locked", "--offline", "--example", "fake_claude", "--profile"])
                .arg(if name == "debug" { "dev" } else { name })
                .arg("--manifest-path")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
                .arg("--target-dir")
                .arg(profile.parent().unwrap())
                .status()
                .unwrap();
            assert!(status.success(), "building fake_claude failed");
            profile.join("examples/fake_claude").to_string_lossy().into_owned()
        })
        .clone()
}
