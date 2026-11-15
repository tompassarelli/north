use north_foundation::test_inputs::git::git_in as git;
use std::path::Path;

use super::super::peer::Peer;
use super::{CHECKED, FLAG, Granted};
use north_foundation::foundation::shim::write_script;

const NORTH: &str = "printf '%s\\n' \"$*\" >> \"$0.calls\"\ncase \"$1 $2\" in\n  'land checks') exit 0 ;;\n  'capacity offload') cat \"$0.host\" 2>/dev/null; exit 0 ;;\nesac\nexit 1\n";

fn granted(lane: &Path, head: &str, turn: &Path) -> Granted {
    let command = "echo ran >> /tmp/outside-the-lane".to_owned();
    let environment = vec![("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()), ("HOME".to_owned(), lane.display().to_string())];
    Granted { session: "w-joining".to_owned(), arguments: [FLAG, CHECKED, "--", "sh", "-c", &command].map(str::to_owned).to_vec(), directory: lane.to_owned(), environment, command, head: head.to_owned(), _turn: std::fs::File::create(turn).unwrap() }
}

/// A finish check joins the run before queueing only on a build host the broker's own placement names: with none, or with this host, the worker's command keeps its domain. Its base is origin/main of the trusted mirror, not of the lane's store, whose origin a worker can point at a repository it made.
pub(in crate::worker_sandbox) fn joins_only_on_a_build_host_the_brokers_own_placement_names() {
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("p");
    let (origin, forged, main, lane, north) = (scratch.path().join("origin.git"), scratch.path().join("forged.git"), project.join("main"), project.join("worktrees/lane"), scratch.path().join("north"));
    std::fs::create_dir_all(&main).unwrap();
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]);
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", &forged.to_string_lossy()]);
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "What origin sent"]);
    let sent = git(&main, &["rev-parse", "HEAD"]);
    git(&main, &["remote", "add", "origin", &origin.to_string_lossy()]);
    git(&main, &["push", "-q", "origin", "main"]);
    north_trusted_mirror::Mirror::open(&main).unwrap();
    git(&main, &["commit", "-q", "--allow-empty", "-m", "A base the worker chose"]);
    git(&main, &["push", "-q", &forged.to_string_lossy(), "main"]);
    git(&main, &["remote", "set-url", "origin", &forged.to_string_lossy()]);
    git(&main, &["worktree", "add", "-q", "-b", "lane", &lane.to_string_lossy()]);
    std::fs::write(lane.join(".safe-push"), "[land]\n\tcheck = ** :: true\n").unwrap();
    git(&lane, &["add", ".safe-push"]);
    git(&lane, &["commit", "-qm", "A head whose checks name no runner"]);
    let head = git(&lane, &["rev-parse", "HEAD"]);
    write_script(&north, NORTH);
    let domain = granted(&lane, &head, &scratch.path().join("turn")).arguments;

    let unplaced = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(unplaced.arguments, domain, "with no build host from the broker's own placement, the worker's command left its domain to run on the broker's host");

    std::fs::write(north.with_extension("host"), north_base::platform::host::name()).unwrap();
    let here = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(here.arguments, domain, "a placement naming this host ran the worker's command outside its domain here");

    std::fs::write(north.with_extension("host"), "builder\n").unwrap();
    let placed = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(placed.arguments, ["land", "checks", "--accept", "echo ran >> /tmp/outside-the-lane", "--base", &sent, "--head", &head, "--runner-host", "builder"], "a joined finish check did not go to the broker's build host alone, on origin/main of the trusted mirror");
}

/// A finish check asked from a process in a scope nested under the session's own scope is the session's, found by its record.
#[test]
fn a_finish_check_from_a_scope_nested_in_the_sessions_own_is_attributed_to_its_record() {
    let scratch = tempfile::tempdir().unwrap();
    let fleet = scratch.path().join("fleet/w-nested");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("host.scope"), "north-session-1352.scope\n").unwrap();
    std::fs::write(fleet.join("session.json"), serde_json::json!({ "name": "w-nested", "cwd": scratch.path().join("code/p/main") }).to_string()).unwrap();
    let host = super::lane_grant::Host { fleet: scratch.path().join("fleet"), home: scratch.path().join("home"), code: Vec::new(), environment: Vec::new(), cgroups: scratch.path().join("cgroup") };
    let cgroup = "0::/user.slice/user@1000.service/app.slice/north-session-1352.scope/tool-7.scope\n";
    let (directory, record) = super::lane_grant::own_session(&Peer::from(Some(cgroup)), &host, "no session").unwrap();
    assert_eq!((directory, record["name"].as_str()), (fleet, Some("w-nested")));
}

/// The laptop compiled a session's finish check `cargo nextest run --workspace` in place twice on 2026-11-14 (north#1553): a finish check that compiles Rust runs as the session's own fleet compile, through the capacity gate's run under the session's owner, which offers it to a build host or refuses it with NO_BUILD_HOST; one that compiles nothing keeps its domain alone, and the worker-domain wrapper never reaches a build host.
#[test]
fn a_finish_check_that_compiles_runs_as_the_sessions_fleet_compile_north_1553() {
    let scratch = tempfile::tempdir().unwrap();
    let north = Path::new("/var/lib/agent-runtime/current/north");
    let check = |command: &str| {
        let plain = granted(scratch.path(), "", &scratch.path().join("turn"));
        Granted { arguments: [FLAG, CHECKED, "--", "sh", "-c", command].map(str::to_owned).to_vec(), command: command.to_owned(), ..plain }.compiling(north).arguments
    };

    let compiled = check("cargo nextest run --workspace");
    let gate = ["capacity", "host", "run", "--class", "heavy", "--owner", "gate:w-joining", "--timeout-seconds", "3600", "--", "/var/lib/agent-runtime/current/north", FLAG, CHECKED, "--", "sh", "-c", "cargo nextest run --workspace"];
    assert_eq!(compiled, gate, "a finish check that compiles Rust escaped the capacity gate's run as the session's fleet compile");
    let command = &compiled[10..];
    assert!(north_capacity_host::capacity::gate::compiles_rust(command), "the gate's run does not read the finish check as a Rust compile, so a host that compiles for no session would run it");
    assert_eq!(north_capacity_host::capacity::build_host_command::for_build_host(command, false), ["north", "depbox", "run", "--", "sh", "-c", "cargo nextest run --workspace"], "a build host was sent the laptop's worker-domain wrapper");

    assert_eq!(check("echo ran"), [FLAG, CHECKED, "--", "sh", "-c", "echo ran"], "a finish check that compiles nothing left its domain for the capacity gate");
    assert_eq!(check("cargo --version"), [FLAG, CHECKED, "--", "sh", "-c", "cargo --version"], "a finish check that only asks cargo its version was held as a compile");

    let joined = Granted { arguments: ["land", "checks", "--accept", "cargo nextest run --workspace"].map(str::to_owned).to_vec(), command: "cargo nextest run --workspace".to_owned(), ..granted(scratch.path(), "", &scratch.path().join("turn")) };
    assert_eq!(joined.compiling(north).arguments[..2], ["land", "checks"], "a finish check that joined the run before queueing was wrapped again");
    let sent = [FLAG, CHECKED, "--", "/var/lib/agent-runtime/current/north", "capacity", "offload", "run", "--host", "gex45", "--base", "aaa", "--head", "bbb", "--timeout", "3600", "--", "cargo nextest run --workspace"].map(str::to_owned).to_vec();
    let offloaded = Granted { arguments: sent.clone(), command: "cargo nextest run --workspace".to_owned(), ..granted(scratch.path(), "", &scratch.path().join("turn")) };
    assert_eq!(offloaded.compiling(north).arguments, sent, "a finish check already sent to a build host was gated again, and the gate refused its runtime path as an absolute path (north#1553, 2026-11-15)");
}
