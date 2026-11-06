use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::lane::{Lane, gather};
use super::remote::invocation;
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
    let command = ["sh".to_owned(), "-c".to_owned(), report.to_owned()];
    ran_command(hosts, lane, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n", &command)
}

fn ran_command(hosts: &Hosts, lane: &Lane, stub_body: &str, command: &[String]) -> Vec<String> {
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, stub_body);
    let (script, words) = invocation(lane, &Job { owner: "t", timeout_seconds: 60, command });
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
fn capacity_an_offloaded_post_land_worktree_outside_the_project_container_rebuilds_from_the_repositorys_main_checkout() {
    let hosts = hosts();
    let lane_dir = hosts.laptop.join("code/proj-data/land-queue/key/post-land-abc");
    git(&hosts.main(), &["worktree", "add", "-q", "--detach", lane_dir.to_str().unwrap(), "origin/main"]);
    write(&lane_dir.join("a.txt"), "landed\n");
    git(&lane_dir, &["commit", "-q", "-am", "landed"]);
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    assert_eq!(lane.repo, "code/proj/main");
    let report = ran_remotely(&hosts, &lane);
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert_eq!(line(&report, "file"), ["a.txt landed"]);
}

// KEEP(bug): on 2026-11-06 the lander's land-check command named the laptop's runtime by absolute path, which ccx33 does not have, so the offloaded run exited 127 (north#662)
#[test]
fn capacity_an_offloaded_command_naming_a_runtime_the_host_lacks_runs_the_hosts_own_north() {
    let hosts = hosts();
    let lane = gather(&hosts.lane("host-north"), &hosts.laptop).unwrap().ship(&[]).unwrap();
    let stub = "printf 'north %s\\n' \"$*\"\n";
    let laptop_only = "/laptop-only/north-data/runtimes/f2ae888b5c28/north".to_owned();
    let command = [laptop_only, "land".to_owned(), "checks".to_owned(), "--leased".to_owned()];
    let report = ran_command(&hosts, &lane, stub, &command);
    assert_eq!(line(&report, "north"), ["capacity host run --class heavy --owner t --timeout-seconds 60 -- ".to_owned() + hosts.remote.join("north-stub").to_str().unwrap() + " land checks --leased"]);
    let present = hosts.remote.join("runtimes/north");
    std::fs::create_dir_all(present.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(&present, "true\n");
    let command = [present.to_str().unwrap().to_owned(), "land".to_owned()];
    let report = ran_command(&hosts, &lane, stub, &command);
    assert!(line(&report, "north")[0].ends_with(&format!("-- {} land", present.display())), "a runtime path that exists on the host stays as given: {report:?}");
}
