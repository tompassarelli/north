use std::fs;
use std::path::Path;
use std::process::Command;

const BUILD_SCRIPT: &str = r#"fn main() {
    let mut seen = String::from("ran\n");
    if std::fs::read_to_string("SECRET").is_ok() {
        seen.push_str("LEAK-token\n");
    }
    if std::net::TcpStream::connect("127.0.0.1:PORT").is_ok() {
        seen.push_str("LEAK-network\n");
    }
    std::fs::write(std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("probe.txt"), seen).unwrap();
}
"#;

fn link(target: &Path, link: &Path) {
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn a_build_script_run_through_the_agent_path_reads_no_credential_and_reaches_no_network() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let secret = home.join(".local/state/north/profiles/claude-a/setup-token");
    fs::create_dir_all(secret.parent().unwrap()).unwrap();
    fs::write(&secret, "sk-ant-oat01-secret\n").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let crate_dir = home.join("code/fixture/main");
    fs::create_dir_all(crate_dir.join("src")).unwrap();
    fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"probe-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n").unwrap();
    fs::write(crate_dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(crate_dir.join("build.rs"), BUILD_SCRIPT.replace("SECRET", &secret.display().to_string()).replace("PORT", &listener.local_addr().unwrap().port().to_string())).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&crate_dir).status().unwrap().success());
    let shims = home.join(".cargo/bin");
    link(Path::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), &shims.join("north"));
    north_core::foundation::shim::copy_executable(crate::support::manifest_dir().join("bin/cargo"), shims.join("cargo"));
    if let Some(rustup) = std::env::var_os("HOME").map(|real| Path::new(&real).join(".rustup")).filter(|rustup| rustup.exists()) {
        link(&rustup, &home.join(".rustup"));
    }
    let output = Command::new(shims.join("cargo"))
        .args(["build", "--offline"])
        .current_dir(&crate_dir)
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env_remove("DEPBOX")
        .env_remove("NORTH_FLEET_SESSION")
        .env_remove("NORTH_FLEET_SCOPE")
        .env_remove("NORTH_WORKER_SANDBOXED")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();
    let seen = fs::read_to_string(crate_dir.join("probe.txt")).unwrap_or_default();
    assert!(output.status.success() && seen.contains("ran"), "the build script did not run: {}", String::from_utf8_lossy(&output.stderr));
    assert!(!seen.contains("LEAK"), "the build script reached what it must not: {seen}");
}
