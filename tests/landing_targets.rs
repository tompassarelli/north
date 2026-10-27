use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(depbox: &Path, checkout: &Path, shared: Option<&Path>, script: &str) -> Output {
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
    command.arg("--").arg(depbox)
        .args(["run", "--", "north-depbox-tool", "-c", script])
        .current_dir(checkout)
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", home.join(".cargo/bin").display(), std::env::var("PATH").unwrap()))
        .env_remove("CARGO_TARGET_DIR");
    if let Some(shared) = shared {
        command.env("CARGO_TARGET_DIR", shared);
    }
    command.output().unwrap()
}

const METADATA: &str = "test ! -w \"$HOME/.cargo/bin\" && exec cargo metadata --offline --no-deps --format-version 1";

fn target(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    metadata["target_directory"].as_str().unwrap().to_owned()
}

fn fake_home(checkout: &Path) -> std::path::PathBuf {
    checkout.with_extension("home")
}

#[test]
fn sandboxed_checkouts_keep_build_outputs_in_their_own_target_directories() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    for name in ["first", "second"] {
        let checkout = scratch.path().join(name);
        let output = run(&depbox, &checkout, None, METADATA);
        assert_eq!(target(&output), checkout.join("target").display().to_string());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn a_shared_cargo_target_under_the_cargo_target_cache_receives_the_build_and_the_checkout_gets_none() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    for (name, relative) in [("lane", ".cache/cargo-target"), ("nested", ".cache/cargo-target/north")] {
        let checkout = scratch.path().join(name);
        let shared = fake_home(&checkout).join(relative);
        let output = run(&depbox, &checkout, Some(&shared), "exec cargo build --offline");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(shared.join("debug/isolation-fixture").is_file(), "{relative}");
        assert!(!checkout.join("target").exists(), "{relative}");
    }
}

#[test]
fn a_cargo_target_outside_the_cargo_target_cache_is_not_bound_or_created_and_the_lane_target_is_used() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let outside = scratch.path().join("outside");
    let rejected = [None, Some(".cache"), Some(".cache/north"), Some(".cache/cargo-target/../escape"), Some(".cargo/bin")];
    for (index, home_relative) in rejected.into_iter().enumerate() {
        let checkout = scratch.path().join(format!("lane{index}"));
        let rejected = home_relative.map_or(outside.clone(), |path| fake_home(&checkout).join(path));
        let output = run(&depbox, &checkout, Some(&rejected), METADATA);
        assert_eq!(target(&output), checkout.join("target").display().to_string(), "{}", rejected.display());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!("depbox: CARGO_TARGET_DIR {} is not under", rejected.display())), "{stderr}");
        assert!(!fake_home(&checkout).join(".cache/escape").exists());
        assert!(!fake_home(&checkout).join(".cache/north").exists());
    }
    assert!(!outside.exists());
}

#[test]
fn a_symlink_inside_the_cargo_target_cache_that_leaves_it_is_not_followed() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("lane");
    let elsewhere = scratch.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let cache = fake_home(&checkout).join(".cache/cargo-target");
    fs::create_dir_all(&cache).unwrap();
    let link = cache.join("link");
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    let output = run(&depbox, &checkout, Some(&link), METADATA);
    assert_eq!(target(&output), checkout.join("target").display().to_string());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not under"));
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn a_relative_cargo_target_or_one_inside_the_checkout_keeps_the_lane_target_without_a_warning() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    for (name, inside) in [("relative", false), ("inside", true)] {
        let checkout = scratch.path().join(name);
        let value = if inside { checkout.join("custom") } else { Path::new("target").to_owned() };
        let output = run(&depbox, &checkout, Some(&value), METADATA);
        assert_eq!(target(&output), checkout.join("target").display().to_string(), "{name}");
        assert!(output.stderr.is_empty(), "{name}");
    }
}
