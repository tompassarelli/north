use std::fs;
use std::path::Path;
use std::process::Command;

const PROBE_BINARY: &str = "fn main() { print!(\"{}\", env!(\"CARGO_MANIFEST_DIR\")); }\n";
const PROBE_TEST: &str = "#[test]
fn runs_what_this_checkout_built() {
    let here = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();
    let binary = std::process::Command::new(env!(\"CARGO_BIN_EXE_probe\")).output().unwrap().stdout;
    assert_eq!((env!(\"CARGO_MANIFEST_DIR\"), String::from_utf8(binary).unwrap().as_str()), (here.as_str(), here.as_str()));
}
";

fn nix_available() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|directory| directory.join("nix").is_file()))
}

fn dev_shell(directory: &Path, target: &Path, command: &[&str]) -> std::process::Output {
    let mut shell = Command::new("nix");
    shell.args(["develop", env!("CARGO_MANIFEST_DIR"), "-c"]).args(command).current_dir(directory).env("CARGO_TARGET_DIR", target);
    for (name, _) in std::env::vars_os().filter(|(name, _)| name.to_string_lossy().starts_with("CARGO_") && name != "CARGO_HOME" && name != "CARGO_TARGET_DIR") {
        shell.env_remove(name);
    }
    shell.env_remove("RUSTFLAGS").env_remove("IN_NIX_SHELL").env_remove("NORTH_DEV_SHELL").output().unwrap()
}

#[test]
fn the_dev_shell_keys_a_shared_target_to_its_checkout_and_leaves_the_checkouts_own_target_unkeyed() {
    if !nix_available() {
        eprintln!("nix is not on PATH; the dev shell is built only on hosts that have it");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let lane = scratch.path().join("lane");
    fs::create_dir_all(lane.join("src")).unwrap();
    fs::create_dir_all(lane.join("tests")).unwrap();
    fs::write(lane.join("Cargo.toml"), "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n").unwrap();
    fs::write(lane.join("src/main.rs"), PROBE_BINARY).unwrap();
    fs::write(lane.join("tests/here.rs"), PROBE_TEST).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).arg(&lane).status().unwrap().success());
    let copy = scratch.path().join("scratch-copy");
    assert!(Command::new("cp").arg("-R").arg(&lane).arg(&copy).status().unwrap().success());
    let shared = scratch.path().join("target");
    for checkout in [&lane, &copy, &lane] {
        let output = dev_shell(checkout, &shared, &["cargo", "test", "--offline", "-q"]);
        assert!(
            output.status.success(),
            "a build from another checkout into a shared target was run as fresh in {}:\n{}{}",
            checkout.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let own = dev_shell(&lane, Path::new("target"), &["sh", "-c", "printf %s \"${RUSTFLAGS:-}\""]);
    assert!(own.status.success(), "{}", String::from_utf8_lossy(&own.stderr));
    assert_eq!(String::from_utf8_lossy(&own.stdout), "", "the checkout's own target is keyed to it");
}
