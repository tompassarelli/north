use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::lane::{Lane, gather};
use super::remote::{Ran, drive, invocation};
use super::script::{Plan, Sink, plan};
use crate::capacity::build_offload::Job;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

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

fn ran_remotely(hosts: &Hosts, lane: &Lane) -> Vec<String> {
    ran_remotely_with(hosts, lane, REPORT)
}

fn ran_remotely_with(hosts: &Hosts, lane: &Lane, report: &str) -> Vec<String> {
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n");
    let command = ["sh".to_owned(), "-c".to_owned(), report.to_owned()];
    let (script, words) = invocation(lane, &Job { owner: "t", timeout_seconds: 60, command: &command });
    let mut child = Command::new("sh").args(["-c", &script, "offload"]).args(&words).env("HOME", &hosts.remote).env("AGENT_CAPACITY_NORTH", &stub).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&lane.bundle).unwrap();
    stdin.write_all(&lane.patch).unwrap();
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success(), "{out}");
    out.lines().skip_while(|line| *line != "NORTH-OFFLOAD-SYNCED").skip(1).map(str::to_owned).collect()
}

fn line<'a>(report: &'a [String], key: &str) -> Vec<&'a str> {
    report.iter().filter_map(|line| line.strip_prefix(key)).map(str::trim_start).collect()
}

#[test]
fn capacity_an_offloaded_lane_with_unpushed_commits_and_uncommitted_changes_runs_against_exactly_the_lane_tree_never_the_hosts_main() {
    let hosts = hosts();
    let lane_dir = hosts.lane("flow-alerts");
    write(&lane_dir.join("a.txt"), "committed lane\n");
    git(&lane_dir, &["commit", "-q", "-am", "unpushed"]);
    write(&lane_dir.join("a.txt"), "dirty lane\n");
    write(&lane_dir.join("b.txt"), "untracked\n");
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    assert_ne!(head, git(&lane_dir, &["rev-parse", "origin/main"]));
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let report = ran_remotely(&hosts, &lane);
    assert_eq!(line(&report, "head"), [head.as_str()]);
    let main = hosts.remote.join("code/proj/main");
    assert_ne!(line(&report, "where"), [main.to_str().unwrap()]);
    assert_eq!(line(&report, "where"), [hosts.remote.join(".cache/build-offload/code-proj-main-flow-alerts").to_str().unwrap()]);
    assert_eq!(line(&report, "status"), ["M a.txt", "?? b.txt"]);
    assert_eq!(line(&report, "file"), ["a.txt dirty lane", "b.txt untracked"]);
    assert_ne!(git(&main, &["rev-parse", "HEAD"]), head);
}

#[test]
fn capacity_a_clean_lane_at_origin_main_still_offloads_to_its_own_checkout_at_that_commit() {
    let hosts = hosts();
    let lane_dir = hosts.lane("clean");
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    assert_eq!(head, git(&lane_dir, &["rev-parse", "origin/main"]));
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let report = ran_remotely(&hosts, &lane);
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert_eq!(line(&report, "status"), Vec::<&str>::new());
    assert_eq!(line(&report, "file"), ["a.txt main"]);
}

#[test]
fn capacity_a_lane_that_cannot_ship_exactly_stays_local_instead_of_running_on_the_hosts_main() {
    let hosts = hosts();
    let lane_dir = hosts.lane("art");
    write(&lane_dir.join("maps/arena.w3x"), "map\n");
    git(&lane_dir, &["add", "maps"]);
    git(&lane_dir, &["commit", "-q", "-m", "map"]);
    let declined = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).err().unwrap();
    assert_eq!((declined.reason(), declined.detail()), ("refused: private asset".to_owned(), "maps/arena.w3x".to_owned()));
}

#[test]
fn capacity_a_command_naming_the_lanes_or_the_main_checkouts_laptop_path_stays_local_because_the_host_keeps_other_trees_there() {
    let hosts = hosts();
    let lane_dir = hosts.lane("flow-alerts");
    let lane_path = lane_dir.to_str().unwrap();
    for command in [format!("cd {lane_path} && cargo test"), format!("cargo test --manifest-path {}/code/proj/main/Cargo.toml", hosts.laptop.display())] {
        let words = ["bash".to_owned(), "-c".to_owned(), command];
        let declined = gather(&lane_dir, &hosts.laptop).unwrap().ship(&words).err().unwrap();
        assert_eq!(declined.reason(), "refused: absolute path");
    }
    let relative = ["cargo".to_owned(), "test".to_owned(), "--manifest-path".to_owned(), "crates/a/Cargo.toml".to_owned()];
    assert!(gather(&lane_dir, &hosts.laptop).unwrap().ship(&relative).is_ok());
}

fn stub(hosts: &Hosts) -> PathBuf {
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n");
    stub
}

fn offloaded(hosts: &Hosts, caller: &Path, script: &str) -> (Plan, Lane, u8) {
    let words = ["bash".to_owned(), "-c".to_owned(), script.to_owned()];
    let plan = plan(&words, caller, &hosts.laptop).unwrap();
    let lane = gather(&plan.directory, &hosts.laptop).unwrap().ship(&plan.command).unwrap();
    let (script, words) = invocation(&lane, &Job { owner: "t", timeout_seconds: 60, command: &plan.command });
    let mut transport = Command::new("sh");
    transport.args(["-c", &script, "offload"]).args(&words).env("HOME", &hosts.remote).env("AGENT_CAPACITY_NORTH", stub(hosts));
    let sink = plan.sink.as_ref().map(|sink| sink.open().unwrap());
    let Ran::Exit(code) = drive(transport, &lane, sink, &|| {}) else { panic!("the lane did not sync") };
    (plan, lane, code)
}

fn tilde(hosts: &Hosts, path: &Path) -> String {
    format!("~/{}", path.strip_prefix(&hosts.laptop).unwrap().display())
}

#[test]
fn capacity_a_command_that_moves_into_another_lane_ships_that_lane_from_any_caller() {
    let hosts = hosts();
    let caller = hosts.lane("caller");
    let other = hosts.lane("other");
    write(&other.join("a.txt"), "other lane\n");
    git(&other, &["commit", "-q", "-am", "other"]);
    let head = git(&other, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(hosts.laptop.join("scratch")).unwrap();
    let log = hosts.laptop.join("scratch/report.log");
    for directory in [caller.clone(), hosts.main()] {
        for target in [other.display().to_string(), tilde(&hosts, &other)] {
            let (plan, lane, code) = offloaded(&hosts, &directory, &format!("cd {target} && {{ {REPORT}; }} > {} 2>&1", log.display()));
            assert_eq!((plan.directory.as_path(), lane.name.as_str(), code), (other.as_path(), "code-proj-main-other", 0), "{} shipped for {target}", directory.display());
            let report: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
            assert_eq!(line(&report, "head"), [head.as_str()]);
            assert_eq!(line(&report, "file"), ["a.txt other lane"]);
        }
    }
    let project = hosts.laptop.join("code/elsewhere/main");
    let words = ["bash".to_owned(), "-c".to_owned(), format!("cd {} && cargo test", tilde(&hosts, &project))];
    let planned = plan(&words, &caller, &hosts.laptop).unwrap();
    assert_eq!(planned.directory, project);
    assert!(gather(&planned.directory, &hosts.laptop).is_err());
}

#[test]
fn capacity_a_command_naming_two_different_checkouts_stays_local() {
    let hosts = hosts();
    let first = hosts.lane("first");
    let second = hosts.lane("second");
    for script in [format!("cd {} && cargo test --manifest-path {}/Cargo.toml", first.display(), second.display()), format!("cd {} && cargo test && cd {} && cargo test", tilde(&hosts, &first), tilde(&hosts, &second))] {
        let words = ["bash".to_owned(), "-c".to_owned(), script.clone()];
        let plan = plan(&words, &hosts.main(), &hosts.laptop).unwrap();
        let declined = gather(&plan.directory, &hosts.laptop).unwrap().ship(&plan.command).err();
        assert_eq!(declined.map(|declined| declined.reason()), Some("refused: absolute path".to_owned()), "{script} shipped");
    }
}

#[test]
fn capacity_a_trailing_top_level_redirect_to_a_laptop_path_is_written_on_the_laptop_not_the_host() {
    let hosts = hosts();
    let lane_dir = hosts.lane("proof");
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(hosts.laptop.join("scratch")).unwrap();
    let log = hosts.laptop.join("scratch/proof.log");
    let (planned, _, code) = offloaded(&hosts, &hosts.main(), &format!("cd {} && {{ {REPORT}; echo stderr-line >&2; }} > {} 2>&1", lane_dir.display(), log.display()));
    assert_eq!((planned.sink, code), (Some(Sink { path: log.clone(), append: false }), 0));
    let written = std::fs::read_to_string(&log).unwrap();
    let report: Vec<String> = written.lines().map(str::to_owned).collect();
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert!(written.contains("stderr-line"), "{written}");
    assert!(!hosts.remote.join(log.strip_prefix(&hosts.laptop).unwrap()).exists());
    let (_, _, code) = offloaded(&hosts, &lane_dir, &format!("echo again >> {}", log.display()));
    assert_eq!(code, 0);
    assert!(std::fs::read_to_string(&log).unwrap().ends_with("stderr-line\nagain\n"));
    for script in ["cargo test > $S/out.log 2>&1".to_owned(), format!("cargo test > {} && true", log.display())] {
        let words = ["bash".to_owned(), "-c".to_owned(), script.clone()];
        let declined = plan(&words, &lane_dir, &hosts.laptop).err();
        assert_eq!(declined.map(|declined| declined.reason()), Some("refused: laptop redirect".to_owned()), "{script} offloaded");
    }
}


const COST: &str = "mkdir -p target; printf 'ship %s\\n' \"$NORTH_OFFLOAD_SHIP_SECONDS\"; printf 'target %s\\n' \"$NORTH_OFFLOAD_SLOT_TARGET\"; printf 'slothead %s\\n' \"$NORTH_OFFLOAD_SLOT_HEAD\"";

#[test]
fn capacity_a_slots_first_offload_is_cold_and_the_next_on_the_same_slot_is_warm_with_its_ship_seconds_in_the_run_environment() {
    use crate::capacity::build_offload::ShipCost;
    let hosts = hosts();
    let lane_dir = hosts.lane("cost");
    let first_head = git(&lane_dir, &["rev-parse", "HEAD"]);
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let cost = |report: &[String]| ShipCost::read(|name| line(report, match name { "NORTH_OFFLOAD_SHIP_SECONDS" => "ship", "NORTH_OFFLOAD_SLOT_TARGET" => "target", _ => "slothead" }).first().map(|value| (*value).to_owned()), "gex45").unwrap();
    let cold = cost(&ran_remotely_with(&hosts, &lane, COST));
    assert_eq!((cold.cache_state.as_str(), cold.slot_head), ("cold", None));
    write(&lane_dir.join("a.txt"), "next\n");
    git(&lane_dir, &["commit", "-q", "-am", "next"]);
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let warm = cost(&ran_remotely_with(&hosts, &lane, COST));
    assert_eq!((warm.cache_state.as_str(), warm.slot_head), ("warm", Some(first_head)));
    assert!(warm.ship_seconds >= 0.0 && warm.ship_seconds < 60.0);
}

#[test]
fn capacity_cd_into_a_worktree_then_cargo_run_from_main_offloads_that_worktrees_tree_not_mains() {
    let hosts = hosts();
    let worktree = hosts.lane("wt");
    write(&worktree.join("a.txt"), "worktree edit\n");
    git(&worktree, &["commit", "-q", "-am", "worktree"]);
    write(&worktree.join("b.txt"), "uncommitted\n");
    let head = git(&worktree, &["rev-parse", "HEAD"]);
    for target in [worktree.display().to_string(), tilde(&hosts, &worktree)] {
        let words = ["bash".to_owned(), "-c".to_owned(), format!("cd {target} && cargo test --locked")];
        let planned = plan(&words, &hosts.main(), &hosts.laptop).unwrap();
        assert_eq!((planned.directory.as_path(), planned.command[2].as_str()), (worktree.as_path(), "cargo test --locked"));
        let lane = gather(&planned.directory, &hosts.laptop).unwrap().ship(&planned.command).unwrap();
        assert_eq!((lane.name.as_str(), lane.head.as_str(), lane.patch.is_empty()), ("code-proj-main-wt", head.as_str(), false), "{target}");
    }
}
