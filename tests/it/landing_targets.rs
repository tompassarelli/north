use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

fn write_checkout(checkout: &Path, modified: SystemTime) {
    let name = checkout.file_name().unwrap().to_string_lossy();
    fs::create_dir_all(checkout.join("src")).unwrap();
    let sources = [(checkout.join("Cargo.toml"), "[package]\nname = \"isolation-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n".to_owned()), (checkout.join("src/main.rs"), format!("fn main() {{ print!(\"{name}\") }}\n"))];
    for (path, text) in sources {
        fs::write(&path, text).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    }
    assert!(Command::new("git").args(["init", "-q"]).current_dir(checkout).status().unwrap().success());
}

fn run(depbox: &Path, checkout: &Path, chosen: Option<&Path>, script: &str) -> Output {
    run_command(depbox, checkout, chosen, "run", &["north-depbox-tool", "-c", script])
}

fn run_command(depbox: &Path, checkout: &Path, chosen: Option<&Path>, mode: &str, command_line: &[&str]) -> Output {
    if !checkout.join(".git").exists() {
        write_checkout(checkout, SystemTime::now());
    }
    let home = fake_home(checkout);
    fs::create_dir_all(home.join(".cargo/bin")).unwrap();
    let tool = home.join(".cargo/bin/north-depbox-tool");
    if !tool.exists() {
        std::os::unix::fs::symlink("/bin/sh", tool).unwrap();
    }
    let north = home.join(".cargo/bin/north");
    if !north.exists() {
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), north).unwrap();
    }
    let rustup = Path::new(&std::env::var_os("HOME").unwrap()).join(".rustup");
    if rustup.exists() && !home.join(".rustup").exists() {
        std::os::unix::fs::symlink(rustup, home.join(".rustup")).unwrap();
    }
    let mut command = Command::new("bwrap");
    command.args(["--bind", "/", "/", "--dev", "/dev", "--tmpfs", "/run", "--dir"])
        .arg(format!("/run/user/{}", rustix::process::getuid().as_raw()));
    if Path::new("/run/current-system").exists() {
        command.args(["--ro-bind", "/run/current-system", "/run/current-system"]);
    }
    command.arg("--").arg(depbox)
        .args([mode, "--"]).args(command_line)
        .current_dir(checkout)
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", home.join(".cargo/bin").display(), std::env::var("PATH").unwrap()))
        .env_remove("DEPBOX")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR");
    if let Some(chosen) = chosen {
        command.env("CARGO_TARGET_DIR", chosen);
    }
    command.output().unwrap()
}

const BUILD: &str = "exec cargo build --offline";

fn fake_home(checkout: &Path) -> std::path::PathBuf {
    checkout.parent().unwrap().join("home")
}

/// The checkout's `target/` as cargo sees it inside depbox, which mounts the checkout again at `~/code/<project>/sandbox`.
#[test]
fn each_checkout_runs_its_own_binary_even_when_its_sources_predate_another_checkouts_build() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let checkouts = [scratch.path().join("first"), scratch.path().join("second")];
    write_checkout(&checkouts[1], SystemTime::now() - Duration::from_secs(3600));
    for checkout in &checkouts {
        let output = run(&depbox, checkout, None, BUILD);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("depbox:"), "{}", String::from_utf8_lossy(&output.stderr));
    }
    for checkout in &checkouts {
        let ran = Command::new(checkout.join("target/debug/isolation-fixture")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&ran.stdout), checkout.file_name().unwrap().to_string_lossy());
    }
    for shared in [".cache/cargo-target", ".cache/cargo-build"] {
        assert!(!fake_home(&checkouts[0]).join(shared).exists(), "{shared}");
    }
}

