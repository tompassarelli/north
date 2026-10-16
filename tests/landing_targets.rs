use std::fs;
use std::path::Path;
use std::process::Command;

fn target(depbox: &Path, checkout: &Path) -> String {
    fs::create_dir_all(checkout.join("src")).unwrap();
    fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"isolation-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n").unwrap();
    fs::write(checkout.join("src/main.rs"), "fn main() {}\n").unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(checkout).status().unwrap().success());
    let home = checkout.with_extension("home");
    fs::create_dir_all(home.join(".cargo/bin")).unwrap();
    std::os::unix::fs::symlink("/bin/sh", home.join(".cargo/bin/north-depbox-tool")).unwrap();
    let rustup = Path::new(&std::env::var_os("HOME").unwrap()).join(".rustup");
    if rustup.exists() {
        std::os::unix::fs::symlink(rustup, home.join(".rustup")).unwrap();
    }
    let mut command = Command::new("bwrap");
    command.args(["--bind", "/", "/", "--dev", "/dev", "--tmpfs", "/run", "--dir"])
        .arg(format!("/run/user/{}", rustix::process::getuid().as_raw()));
    if Path::new("/run/current-system").exists() {
        command.args(["--ro-bind", "/run/current-system", "/run/current-system"]);
    }
    let output = command.arg("--").arg(depbox)
        .args(["run", "--", "north-depbox-tool", "-c", "test ! -w \"$HOME/.cargo/bin\" && exec cargo metadata --offline --no-deps --format-version 1"])
        .current_dir(checkout)
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", home.join(".cargo/bin").display(), std::env::var("PATH").unwrap()))
        .env("CARGO_TARGET_DIR", Path::new(&std::env::var_os("HOME").unwrap()).join(".cache/cargo-target/north"))
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    metadata["target_directory"].as_str().unwrap().to_owned()
}

#[test]
fn sandboxed_checkouts_keep_build_outputs_in_their_own_target_directories() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        let checkout = scratch.path().join(name);
        assert_eq!(target(&depbox, &checkout), checkout.join("target").display().to_string());
    }
}
