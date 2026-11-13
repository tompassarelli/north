use super::quote;
use super::script::run_part;
use north_runtime_follow::layout::{self, Layout, TREE};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const SKEWED_CURRENT: &str = "echo \"remote-current refuses: $*\" >&2\nexit 2\n";
const LANDER_NORTH: &str = "if [ \"$1\" = capacity ]; then shift 10; exec \"$@\"; fi\nif [ \"$1\" = dev-shell ]; then shift 5; exec \"$@\"; fi\necho \"lander $*\"\n";
const TREE_READING_NORTH: &str = "if [ \"$1\" = capacity ]; then shift 10; exec \"$@\"; fi\nif [ \"$1\" = dev-shell ]; then shift 5; exec \"$@\"; fi\nhere=$(dirname \"$0\")\ncat \"$here/tree/harness/defaults.toml\" || exit 101\nexit 3\n";
const LANDER_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn runtimes(home: &Path) -> PathBuf {
    home.join(".local/state/north/runtimes")
}

fn place_runtime(home: &Path, sha: &str, north: &str, with_tree: bool) -> PathBuf {
    let runtime = runtimes(home).join(sha);
    fs::create_dir_all(&runtime).unwrap();
    north_base::foundation::shim::write_script(&runtime.join("north"), north);
    if with_tree {
        let marker = runtime.join(layout::tree_marker());
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(marker, format!("runtime = \"{sha}\"\n")).unwrap();
    }
    runtime
}

fn build_host(lander_runtime: Option<bool>) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/bin")).unwrap();
    north_base::foundation::shim::write_script(&home.path().join(".local/bin/north"), SKEWED_CURRENT);
    if let Some(with_tree) = lander_runtime {
        place_runtime(home.path(), LANDER_SHA, LANDER_NORTH, with_tree);
    }
    home
}

fn script_on(home: &Path, slot: &Path, runtime: &str, command: &str) -> Command {
    let head = format!("set -eu\nslot={} command={} limit=30 base=b head=h landing= landref= runtime={}\nproved() {{ :; }}\n{}", quote(&slot.display().to_string()), quote(command), quote(runtime), run_part());
    let mut script = Command::new("sh");
    script.args(["-c", &head]).env("HOME", home).env("TMPDIR", home);
    script
}

fn run_on(home: &Path, runtime: &str) -> Output {
    let slot = tempfile::tempdir().unwrap();
    script_on(home, slot.path(), runtime, "north check").output().unwrap()
}

#[test]
fn every_north_the_offloaded_command_and_its_wrapper_call_is_the_landers_runtime_not_the_build_hosts_newer_current() {
    let home = build_host(Some(true));
    let skewed = run_on(home.path(), "");
    assert_eq!(skewed.status.code(), Some(2), "without the pin the build host's current is what runs");
    assert!(String::from_utf8_lossy(&skewed.stderr).contains("remote-current refuses: capacity host run"));
    let pinned = run_on(home.path(), LANDER_SHA);
    assert!(pinned.status.success(), "{}", String::from_utf8_lossy(&pinned.stderr));
    assert_eq!(String::from_utf8_lossy(&pinned.stdout), "lander check\n");
    assert!(!String::from_utf8_lossy(&pinned.stderr).contains("remote-current"));
    let treeless = build_host(Some(false));
    let refused = run_on(treeless.path(), LANDER_SHA);
    assert_eq!(refused.status.code(), Some(i32::from(super::TRANSPORT_FAILED)), "{}", String::from_utf8_lossy(&refused.stderr));
    assert!(String::from_utf8_lossy(&refused.stderr).contains(&format!("lacks north or {}", layout::tree_marker())));
    assert!(!String::from_utf8_lossy(&refused.stdout).contains("lander check"));
}

fn wait_for(path: &Path) {
    assert!(north_base::platform::wait::until(Duration::from_secs(20), || path.exists()), "{} never appeared", path.display());
}

#[test]
fn a_check_started_on_runtime_a_finishes_on_a_with_its_real_verdict_while_b_is_promoted_and_a_is_collected_mid_run() {
    let home = build_host(None);
    let shas: Vec<String> = ('a'..='f').map(|digit| digit.to_string().repeat(40)).collect();
    let a = place_runtime(home.path(), &shas[0], TREE_READING_NORTH, true);
    fs::write(a.join(layout::LEASE), "").unwrap();
    for sha in &shas[1..] {
        place_runtime(home.path(), sha, LANDER_NORTH, true);
    }
    for (index, sha) in shas.iter().enumerate() {
        let at = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600 + index as u64);
        fs::File::open(runtimes(home.path()).join(sha)).unwrap().set_modified(at).unwrap();
    }
    let b = shas.last().unwrap();
    let state = Layout { state: home.path().join(".local/state/north") };
    std::os::unix::fs::symlink(layout::link(&shas[0]), state.current()).unwrap();
    let slot = tempfile::tempdir().unwrap();
    let started = slot.path().join("started");
    let resume = slot.path().join("resume");
    let command = format!("touch {}; while [ ! -e {} ]; do sleep 0.05; done; north check", quote(&started.display().to_string()), quote(&resume.display().to_string()));
    let check = script_on(home.path(), slot.path(), &shas[0], &command).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    wait_for(&started);
    fs::remove_file(state.current()).unwrap();
    std::os::unix::fs::symlink(layout::link(b), state.current()).unwrap();
    let processes = tempfile::tempdir().unwrap();
    let checking = processes.path().join("42");
    fs::create_dir_all(&checking).unwrap();
    fs::write(checking.join("cmdline"), format!("sh\0-c\0north check\0{}\0", a.join("north").display())).unwrap();
    north_runtime_follow::collection::collect(&state, b, None, processes.path(), &[]).unwrap();
    assert!(a.join(TREE).is_dir(), "collection removed the runtime a running check holds");
    fs::write(&resume, "").unwrap();
    let finished = check.wait_with_output().unwrap();
    assert_eq!(finished.status.code(), Some(3), "{}", String::from_utf8_lossy(&finished.stderr));
    assert_eq!(String::from_utf8_lossy(&finished.stdout), format!("runtime = \"{}\"\n", shas[0]));
    fs::remove_dir_all(&checking).unwrap();
    north_runtime_follow::collection::collect(&state, b, None, processes.path(), &[]).unwrap();
    assert!(!a.exists(), "with its check finished, runtime a is collected like any old sibling");
}
