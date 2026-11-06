use super::{quote, run_part};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const SKEWED_CURRENT: &str = "echo \"remote-current refuses: $*\" >&2\nexit 2\n";
const LANDER_NORTH: &str = "if [ \"$1\" = capacity ]; then shift 5; exec \"$@\"; fi\nif [ \"$1\" = dev-shell ]; then shift 5; exec \"$@\"; fi\necho \"lander $*\"\n";
const LANDER_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn build_host(with_lander_runtime: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/bin")).unwrap();
    crate::foundation::shim::write_script(&home.path().join(".local/bin/north"), SKEWED_CURRENT);
    if with_lander_runtime {
        let runtime = home.path().join(".local/state/north/runtimes").join(LANDER_SHA);
        fs::create_dir_all(&runtime).unwrap();
        crate::foundation::shim::write_script(&runtime.join("north"), LANDER_NORTH);
    }
    home
}

fn run_on(home: &Path, runtime: &str) -> Output {
    let slot = tempfile::tempdir().unwrap();
    let script = run_part();
    let head = format!("set -eu\nslot={} command='north check' limit=10 base=b head=h landing= landref= runtime={}\n{script}", quote(&slot.path().display().to_string()), quote(runtime));
    Command::new("sh").args(["-c", &head]).env("HOME", home).output().unwrap()
}

#[test]
fn every_north_the_offloaded_command_and_its_wrapper_call_is_the_landers_runtime_not_the_build_hosts_newer_current() {
    let home = build_host(true);
    let skewed = run_on(home.path(), "");
    assert_eq!(skewed.status.code(), Some(2), "without the pin the build host's current is what runs");
    assert!(String::from_utf8_lossy(&skewed.stderr).contains("remote-current refuses: capacity deadline"));
    let pinned = run_on(home.path(), LANDER_SHA);
    assert!(pinned.status.success(), "{}", String::from_utf8_lossy(&pinned.stderr));
    assert_eq!(String::from_utf8_lossy(&pinned.stdout), "lander check\n");
    assert!(!String::from_utf8_lossy(&pinned.stderr).contains("remote-current"));
}

