use north_foundation::test_inputs::git::git_in as git;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::lane::{Lane, gather};
use super::remote::{Ran, drive, invocation};
use super::take::{release_command, transport};
use north_capacity::capacity::build_offload::Job;
use north_capacity::capacity::handover::Offer;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct Hosts {
    _scratch: tempfile::TempDir,
    laptop: PathBuf,
    remote: PathBuf,
}

impl Hosts {
    fn main(&self) -> PathBuf {
        self.laptop.join("code/proj/main")
    }

    fn lane(&self, name: &str) -> PathBuf {
        let lane = self.laptop.join("code/proj/worktrees").join(name);
        git(&self.main(), &["worktree", "add", "-q", "-b", name, lane.to_str().unwrap(), "origin/main"]);
        lane
    }
}

fn hosts() -> Hosts {
    let scratch = tempfile::tempdir().unwrap();
    let (laptop, remote, origin) = (scratch.path().join("laptop"), scratch.path().join("gex45"), scratch.path().join("origin.git"));
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    let main = laptop.join("code/proj/main");
    git(scratch.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["checkout", "-q", "-B", "main"]);
    write(&main.join("a.txt"), "main\n");
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "seed"]);
    write(&main.join("c.txt"), "second\n");
    git(&main, &["add", "c.txt"]);
    git(&main, &["commit", "-q", "-m", "second"]);
    git(&main, &["push", "-q", "origin", "main"]);
    std::fs::create_dir_all(remote.join("code/proj")).unwrap();
    git(scratch.path(), &["clone", "-q", origin.to_str().unwrap(), remote.join("code/proj/main").to_str().unwrap()]);
    Hosts { _scratch: scratch, laptop, remote }
}

const REPORT: &str = "printf 'head %s\\n' \"$(git rev-parse HEAD)\"; printf 'where %s\\n' \"$(pwd)\"; git status --porcelain | sed 's/^/status /'; for f in a.txt b.txt; do [ -e $f ] && printf 'file %s %s\\n' $f \"$(cat $f)\"; done; true";

fn ran_remotely_with(hosts: &Hosts, lane: &Lane, report: &str) -> Vec<String> {
    let command = ["sh".to_owned(), "-c".to_owned(), report.to_owned()];
    ran_command(hosts, lane, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n", &command)
}

fn ran_command(hosts: &Hosts, lane: &Lane, stub_body: &str, command: &[String]) -> Vec<String> {
    let stub = hosts.remote.join("north-stub");
    north_foundation::foundation::shim::write_script(&stub, stub_body);
    let returned = "NORTH-OFFLOAD-RETURNED-test";
    let (script, words) = invocation(lane, &Job { owner: "t", timeout_seconds: 60, command, cpus: 6.0 }, returned);
    let mut transport = Command::new("sh");
    transport.args(["-c", &script, "offload"]).args(&words).env("HOME", &hosts.remote).env("AGENT_CAPACITY_NORTH", &stub);
    let output = hosts.remote.join("output.log");
    let ran = drive(transport, lane, Some(std::fs::File::create(&output).unwrap()), &|| true, returned);
    assert!(matches!(ran, Ran::Exit(0)));
    std::fs::read_to_string(&output).unwrap().lines().map(str::to_owned).collect()
}

fn line<'a>(report: &'a [String], key: &str) -> Vec<&'a str> {
    report.iter().filter_map(|line| line.strip_prefix(key)).map(str::trim_start).collect()
}

#[test]
fn capacity_an_offloaded_lane_with_unpushed_commits_and_uncommitted_changes_runs_against_exactly_the_lane_tree_never_the_hosts_main() {
    let hosts = hosts();
    let lane_dir = hosts.lane("flow-alerts");
    write(&lane_dir.join("a.txt"), "committed lane\n");
    write(&lane_dir.join("Cargo.lock"), "resolved before\n");
    write(&lane_dir.join("crates/x/Cargo.lock"), "nested before\n");
    git(&lane_dir, &["add", "Cargo.lock", "crates/x/Cargo.lock"]);
    git(&lane_dir, &["commit", "-q", "-am", "unpushed"]);
    write(&lane_dir.join("a.txt"), "dirty lane\n");
    write(&lane_dir.join("b.txt"), "untracked\n");
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    assert_ne!(head, git(&lane_dir, &["rev-parse", "origin/main"]));
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let report = ran_remotely_with(&hosts, &lane, &format!("{REPORT}; printf 'resolved after\\n' >Cargo.lock"));
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert_eq!(std::fs::read_to_string(lane_dir.join("Cargo.lock")).unwrap(), "resolved after\n", "a lock the run changed comes back to the lane (north#671)");
    assert_eq!(std::fs::read_to_string(lane_dir.join("crates/x/Cargo.lock")).unwrap(), "nested before\n");
    let main = hosts.remote.join("code/proj/main");
    assert_ne!(line(&report, "where"), [main.to_str().unwrap()]);
    assert_eq!(line(&report, "where"), [hosts.remote.join(".cache/build-offload/code-proj-main-flow-alerts").to_str().unwrap()]);
    assert_eq!(line(&report, "status"), ["M a.txt", "?? b.txt"]);
    assert_eq!(line(&report, "file"), ["a.txt dirty lane", "b.txt untracked"]);
    assert_ne!(git(&main, &["rev-parse", "HEAD"]), head);
}

#[test]
fn capacity_a_finish_check_the_broker_wraps_in_the_worker_domain_ships_while_a_laptop_path_inside_it_is_still_refused_north_1553() {
    let hosts = hosts();
    let lane_dir = hosts.lane("wrapped");
    let runtime = hosts.laptop.join("code/proj-data/runtimes/abc/north").to_string_lossy().into_owned();
    let wrapped = |inner: &str| [runtime.as_str(), "--worker-sandbox", "--checked", "--", "sh", "-c", inner].map(str::to_owned);
    assert!(gather(&lane_dir, &hosts.laptop).unwrap().ship(&wrapped("cargo test -p x")).is_ok(), "the broker's worker-domain wrapper names the laptop runtime, which refused every compiling finish check as an absolute path");
    let inner = format!("cargo test --manifest-path {}/Cargo.toml", lane_dir.display());
    assert!(gather(&lane_dir, &hosts.laptop).unwrap().ship(&wrapped(&inner)).is_err());
}

#[test]
fn capacity_the_taker_never_rides_the_shared_ssh_master() {
    let offer = Offer {
        repo: "code/north/main".to_owned(),
        head: "a".repeat(40),
        name: "code-north-main-lane".to_owned(),
        subdirectory: String::new(),
        owner: "worker".to_owned(),
        timeout_seconds: 60,
        command: vec!["cargo".to_owned(), "test".to_owned()],
        bundle_bytes: 1,
        patch_bytes: 0,
        returned: "marker".to_owned(),
    };
    for command in [release_command("tom@gex45"), transport(&offer, Some("tom@ccx33"))] {
        let words: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
        assert_eq!(command.get_program(), "ssh");
        let control = words.iter().find_map(|word| word.strip_prefix("ControlPath=")).unwrap_or_default();
        assert!(control.ends_with("/north-taker-%C") && !control.contains("north-ssh-"), "{words:?}");
        assert!(words.windows(2).any(|pair| pair == ["-o", "ControlMaster=auto"]), "{words:?}");
    }
}

#[test]
fn capacity_a_pulled_run_executes_in_a_sandbox_without_credentials_or_network_and_writes_only_its_lane() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for secret in [".ssh/id_ed25519", ".config/sops/age/keys.txt", ".config/gh/hosts.yml"] {
        write(&home.join(secret), "secret\n");
    }
    let lane = home.join(".cache/build-offload/code-proj-main-pulled");
    std::fs::create_dir_all(&lane).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        "echo inside; test -e ~/.ssh && echo LEAK-ssh; test -e ~/.config/sops && echo LEAK-sops; test -e ~/.config/gh && echo LEAK-gh; [ -n \"${{GH_TOKEN-}}\" ] && echo LEAK-token; (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo LEAK-net; touch written; echo done"
    );
    let north = std::env::current_exe().unwrap();
    let layout = north_capacity::capacity::sandbox::Layout::of(&lane, &lane, &home, &north);
    let output = layout.command(&["bash".to_owned(), "-c".to_owned(), probe]).env("GH_TOKEN", "gho_secret").output().unwrap();
    let seen = String::from_utf8_lossy(&output.stdout);
    assert!(!seen.contains("LEAK"), "the sandboxed command reached what it must not: {seen}");
    if seen.contains("inside") {
        assert!(seen.contains("done") && lane.join("written").is_file(), "the lane stays writable: {seen} {}", String::from_utf8_lossy(&output.stderr));
    } else {
        assert!(!output.status.success() && !lane.join("written").exists(), "a host that cannot build the sandbox refuses the run: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(super::remote::script(super::remote::Trust::Pulled).contains(r#""$north" capacity sandbox --lane "$slot" -- "$@""#), "the taker's lane script runs the job through the sandbox");
}

