use std::path::Path;
use std::process::Command;

use super::{CHECKED, FLAG, Granted};
use crate::foundation::shim::write_script;

const NORTH: &str = "printf '%s\\n' \"$*\" >> \"$0.calls\"\ncase \"$1 $2\" in\n  'land checks') exit 0 ;;\n  'capacity offload') cat \"$0.host\" 2>/dev/null; exit 0 ;;\nesac\nexit 1\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_AUTHOR_NAME", "a").env("GIT_AUTHOR_EMAIL", "a@a").env("GIT_COMMITTER_NAME", "a").env("GIT_COMMITTER_EMAIL", "a@a").output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn granted(lane: &Path, head: &str, turn: &Path) -> Granted {
    let command = "echo ran >> /tmp/outside-the-lane".to_owned();
    let environment = vec![("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()), ("HOME".to_owned(), lane.display().to_string())];
    Granted { session: "w-joining".to_owned(), arguments: [FLAG, CHECKED, "--", "sh", "-c", &command].map(str::to_owned).to_vec(), directory: lane.to_owned(), environment, command, head: head.to_owned(), _turn: std::fs::File::create(turn).unwrap() }
}

/// A finish check joins the run before queueing only on a build host the broker's own placement names: with none, or with this host, the worker's command keeps its domain.
pub(in crate::worker_sandbox) fn joins_only_on_a_build_host_the_brokers_own_placement_names() {
    let scratch = tempfile::tempdir().unwrap();
    let (lane, north) = (scratch.path().join("lane"), scratch.path().join("north"));
    std::fs::create_dir_all(&lane).unwrap();
    git(&lane, &["init", "-q", "-b", "lane"]);
    std::fs::write(lane.join(".safe-push"), "[land]\n\tcheck = ** :: true\n").unwrap();
    git(&lane, &["add", ".safe-push"]);
    git(&lane, &["commit", "-qm", "A head whose checks name no runner"]);
    let head = git(&lane, &["rev-parse", "HEAD"]);
    git(&lane, &["update-ref", "refs/remotes/origin/main", &head]);
    write_script(&north, NORTH);
    let domain = granted(&lane, &head, &scratch.path().join("turn")).arguments;

    let unplaced = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(unplaced.arguments, domain, "with no build host from the broker's own placement, the worker's command left its domain to run on the broker's host");

    std::fs::write(north.with_extension("host"), crate::platform::host::name()).unwrap();
    let here = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(here.arguments, domain, "a placement naming this host ran the worker's command outside its domain here");

    std::fs::write(north.with_extension("host"), "builder\n").unwrap();
    let placed = granted(&lane, &head, &scratch.path().join("turn")).joining(&north);
    assert_eq!(placed.arguments, ["land", "checks", "--accept", "echo ran >> /tmp/outside-the-lane", "--base", &head, "--head", &head, "--runner-host", "builder"], "a joined finish check did not go to the broker's build host alone, on the destination's tip");
}
