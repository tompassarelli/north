use north_core::foundation::shim;
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

const METADATA: &str = "test ! -w \"$HOME/.cargo/bin\" && exec cargo metadata --offline --no-deps --format-version 1";
const BUILD: &str = "exec cargo build --offline";

fn target(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    metadata["target_directory"].as_str().unwrap().to_owned()
}

fn fake_home(checkout: &Path) -> std::path::PathBuf {
    checkout.parent().unwrap().join("home")
}

/// The checkout's `target/` as cargo sees it inside depbox, which mounts the checkout again at `~/code/<project>/sandbox`.
fn sandbox_target(checkout: &Path) -> String {
    let project = checkout.parent().unwrap().file_name().unwrap();
    fake_home(checkout).join("code").join(project).join("sandbox/target").display().to_string()
}

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

#[test]
fn a_relative_cargo_target_naming_the_checkout_target_is_used_without_a_warning() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("relative");
    let output = run(&depbox, &checkout, Some(Path::new("target")), METADATA);
    assert_eq!(target(&output), sandbox_target(&checkout));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn any_other_cargo_target_is_ignored_with_a_warning_and_never_created() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let outside = scratch.path().join("outside");
    let checkout = scratch.path().join("lane");
    let home = fake_home(&checkout);
    for chosen in [outside, checkout.join("custom"), home.join(".cache/cargo-target"), home.join(".cache/cargo-target/north")] {
        let output = run(&depbox, &checkout, Some(&chosen), METADATA);
        assert_eq!(target(&output), sandbox_target(&checkout), "{}", chosen.display());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!("depbox: ignoring CARGO_TARGET_DIR {}", chosen.display())), "{stderr}");
        assert!(!chosen.exists(), "{}", chosen.display());
    }
}

#[test]
fn a_run_depbox_inside_a_run_depbox_runs_directly_and_a_fetch_inside_it_refuses_naming_the_outer_sandbox() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("nested");
    let script = format!("'{0}' run -- sh -c 'echo \"$DEPBOX\"' && exec '{0}' fetch -- true", depbox.display());
    let output = run(&depbox, &checkout, None, &script);
    let top = checkout.canonicalize().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("run:{}\n", top.display()));
    assert!(stderr.contains(&format!("depbox: already inside the depbox run sandbox of {}", top.display())), "{stderr}");
}

#[test]
fn a_command_under_a_hidden_tmp_path_is_refused_with_the_path_and_the_binds() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let script = elsewhere.path().join("x.sh");
    shim::write_script(&script, "exit 0\n");
    let checkout = scratch.path().join("hidden");
    let output = run_command(&depbox, &checkout, None, "run", &[script.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(&format!("depbox: {} is hidden inside the sandbox", script.display())), "{stderr}");
    assert!(stderr.contains(&checkout.canonicalize().unwrap().display().to_string()), "{stderr}");
}

#[test]
fn an_unfetched_cargo_lock_names_the_fetch_command_until_a_cargo_fetch_succeeds() {
    let depbox = std::env::current_dir().unwrap().join("bin/depbox");
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("locked");
    write_checkout(&checkout, SystemTime::now());
    fs::write(checkout.join("Cargo.lock"), "version = 4\n").unwrap();
    let fetch = "depbox fetch -- cargo fetch --locked";
    let before = run(&depbox, &checkout, None, "exit 0");
    assert!(String::from_utf8_lossy(&before.stderr).contains(fetch), "{}", String::from_utf8_lossy(&before.stderr));
    shim::write_script(&fake_home(&checkout).join(".cargo/bin/cargo"), "exit 0\n");
    let fetched = run_command(&depbox, &checkout, None, "fetch", &["cargo", "fetch", "--locked"]);
    assert!(fetched.status.success(), "{}", String::from_utf8_lossy(&fetched.stderr));
    let after = run(&depbox, &checkout, None, "exit 0");
    assert!(after.status.success() && !String::from_utf8_lossy(&after.stderr).contains(fetch), "{}", String::from_utf8_lossy(&after.stderr));
}
