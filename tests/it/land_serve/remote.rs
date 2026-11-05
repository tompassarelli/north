use std::fs;
use north_landing::land::queue;

use super::{Fixture, git, json_names};

#[test]
fn remote_outbox_lands() {
    let mut fixture = Fixture::new();
    fixture.landing_config("\tlander-host = lander.invalid\n\tremote-hosts = gex45\n");
    let home = fixture.root.join("remote-homes/gex45");
    let clone = fixture.root.join("gex45/proj/main");
    fs::create_dir_all(&home).unwrap();
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    git(&clone, &["config", "user.name", "Remote scenario"]);
    git(&clone, &["config", "user.email", "remote@example.invalid"]);
    let lane = fixture.root.join("gex45/proj/worktrees/r");
    git(&clone, &["worktree", "add", "-q", "-b", "lane-r", lane.to_str().unwrap(), "origin/main"]);
    fixture.commit(&lane, "r.txt", "r", "change from r");
    let head = git(&lane, &["rev-parse", "HEAD"]);
    let top = git(&lane, &["rev-parse", "--show-toplevel"]);
    let queued = fixture.land_enqueue(&lane, &home.join(".local/state"), "session-remote");
    assert!(queued.status.success());
    assert!(String::from_utf8_lossy(&queued.stdout).contains(&format!("Done: queued (outbox) {top} lane-r {head}")));
    let outbox = queue::directory(&home.join(".local/state"), fixture.origin.to_str().unwrap()).join(queue::OUTBOX);
    let names = json_names(&outbox);
    assert_eq!(names.len(), 1, "{names:?}");

    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"]);
    assert!(json_names(&outbox).is_empty(), "the remote ticket is still in the outbox");
    assert_eq!(json_names(&outbox.join("taken")), names);
    let main = fixture.main();
    let local = fixture.queue().join(&names[0]);
    assert_eq!(fixture.verdict(&local), Some(format!("landed {main}")));
    let remote = queue::read(&outbox.join("taken").join(&names[0])).unwrap();
    let done = queue::read(&fixture.queue().join("done").join(&names[0])).unwrap();
    assert_eq!((done.host.as_str(), done.root.as_path()), (remote.host.as_str(), remote.root.as_path()), "the pulled ticket keeps its host and root");
    assert!(fixture.fleet().contains(&format!("fleet send --from service:north-land session-remote landed lane-r as {main}")), "{:?}", fixture.fleet());
    let ssh = fixture.read("ssh");
    assert!(ssh.lines().all(|line| line.contains("ConnectTimeout=5")), "{ssh}");
    assert!(ssh.lines().any(|line| line.contains("git-upload-pack") && line.contains(&top)), "the lane is fetched from ssh://gex45{top}: {ssh}");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from r"], "a taken ticket was pulled again");
}

impl Fixture {
}

