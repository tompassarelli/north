use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::queue::{self, Ticket};
use north_landing::land::repositories::{self, Mapping};

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct World {
    _scratch: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
}

impl World {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_owned();
        let state = root.join("state");
        fs::create_dir_all(&state).unwrap();
        Self { _scratch: scratch, root, state }
    }

    fn checkout(&self, name: &str) -> (PathBuf, String) {
        let origin = self.root.join(format!("{name}.git"));
        git(&self.root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        let checkout = self.root.join(name).join("main");
        fs::create_dir_all(&checkout).unwrap();
        git(&checkout, &["init", "-q", "-b", "main"]);
        git(&checkout, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&checkout, &["config", "user.name", "Mapping scenario"]);
        git(&checkout, &["config", "user.email", "mapping@example.invalid"]);
        git(&checkout, &["config", "commit.gpgsign", "false"]);
        git(&checkout, &["commit", "-q", "--allow-empty", "-m", "base"]);
        (checkout, origin.to_str().unwrap().to_owned())
    }

    fn ticket(&self, origin: &str, root: &Path, micros: u128, host: &str, done: bool) -> PathBuf {
        let ticket = Ticket {
            origin: origin.to_owned(),
            root: root.to_owned(),
            host: host.to_owned(),
            branch: "lane".to_owned(),
            head: "0".repeat(40),
            session: None,
            queued_at: "2026-11-05T10:00:00Z".to_owned(),
            first_commit_secs: None,
            red_fix: None,
            verdict: None,
            keep_lane: false,
        };
        let directory = queue::directory(&self.state, origin);
        let path = if done { directory.join("done") } else { directory }.join(queue::file_name(micros, "lane"));
        queue::write(&path, &ticket).unwrap();
        path
    }

    fn queue(&self, origin: &str) -> PathBuf {
        queue::directory(&self.state, origin)
    }
}

#[test]
fn a_queue_maps_to_the_checkout_its_ticket_root_belongs_to() {
    let world = World::new();
    let (checkout, origin) = world.checkout("south");
    let lane = checkout.parent().unwrap().join("worktrees/lane");
    git(&checkout, &["worktree", "add", "-q", "--detach", lane.to_str().unwrap()]);
    world.ticket(&origin, &lane, 10, "here", false);
    assert_eq!(repositories::map(&world.queue(&origin), "here"), Mapping::Checkout(checkout));
}

#[test]
fn a_queue_whose_ticket_roots_are_gone_is_unmapped_and_named() {
    let world = World::new();
    let (_, origin) = world.checkout("south");
    world.ticket(&origin, &world.root.join("south/worktrees/gone"), 10, "here", false);
    let Mapping::Unmapped(reason) = repositories::map(&world.queue(&origin), "here") else { panic!("a missing root mapped") };
    assert!(reason.contains("gone") && reason.contains("no longer exists"), "{reason}");
    let found = repositories::discovered(&world.state, "here");
    assert!(found.checkouts.is_empty());
    assert_eq!(found.unserved.len(), 1, "a queued ticket with no checkout is reported, not dropped silently: {found:?}");
}

#[test]
fn an_empty_unmapped_queue_is_not_reported() {
    let world = World::new();
    let (_, origin) = world.checkout("south");
    world.ticket(&origin, &world.root.join("south/worktrees/gone"), 10, "here", true);
    let found = repositories::discovered(&world.state, "here");
    assert_eq!(found, repositories::Discovery::default());
}

#[test]
fn two_origins_map_to_two_checkouts() {
    let world = World::new();
    let (north, north_origin) = world.checkout("north");
    let (south, south_origin) = world.checkout("south");
    world.ticket(&north_origin, &north, 10, "here", false);
    world.ticket(&south_origin, &south, 11, "here", false);
    assert_ne!(world.queue(&north_origin), world.queue(&south_origin));
    let found = repositories::discovered(&world.state, "here");
    let mut expected = vec![north, south];
    expected.sort();
    assert_eq!(found.checkouts, expected);
    assert!(found.unserved.is_empty());
}

#[test]
fn a_ticket_from_another_host_never_maps_a_local_path() {
    let world = World::new();
    let (checkout, origin) = world.checkout("south");
    world.ticket(&origin, &checkout, 10, "gex45", false);
    let Mapping::Unmapped(reason) = repositories::map(&world.queue(&origin), "here") else { panic!("a remote ticket mapped") };
    assert!(reason.contains("holds no ticket from here"), "{reason}");
}

#[test]
fn a_renamed_checkout_follows_its_newest_ticket() {
    let world = World::new();
    let (checkout, origin) = world.checkout("south");
    world.ticket(&origin, &world.root.join("old-south/main"), 10, "here", true);
    world.ticket(&origin, &checkout, 20, "here", true);
    assert_eq!(repositories::map(&world.queue(&origin), "here"), Mapping::Checkout(checkout));
    let renamed = world.root.join("south-renamed");
    fs::rename(world.root.join("south"), &renamed).unwrap();
    let Mapping::Unmapped(reason) = repositories::map(&world.queue(&origin), "here") else { panic!("a vanished checkout mapped") };
    assert!(reason.contains("no longer exists"), "{reason}");
    world.ticket(&origin, &renamed.join("main"), 30, "here", false);
    assert_eq!(repositories::map(&world.queue(&origin), "here"), Mapping::Checkout(renamed.join("main")));
}

#[test]
fn a_checkout_that_now_pushes_elsewhere_is_not_mapped_to_the_old_queue() {
    let world = World::new();
    let (checkout, origin) = world.checkout("south");
    world.ticket(&origin, &checkout, 10, "here", false);
    git(&checkout, &["remote", "set-url", "--push", "origin", "/elsewhere/south.git"]);
    let Mapping::Unmapped(reason) = repositories::map(&world.queue(&origin), "here") else { panic!("a re-pointed checkout mapped") };
    assert!(reason.contains("pushes to queue"), "{reason}");
}

#[test]
fn a_repository_that_keeps_its_git_directory_elsewhere_is_unservable() {
    let world = World::new();
    let (checkout, _) = world.checkout("south");
    let bare = world.root.join("south.git");
    let linked = world.root.join("linked");
    git(&checkout, &["worktree", "add", "-q", "--detach", linked.to_str().unwrap()]);
    assert_eq!(repositories::servable(&linked).unwrap(), checkout);
    git(&world.root, &["clone", "-q", "--bare", bare.to_str().unwrap(), world.root.join("kept.git").to_str().unwrap()]);
    let error = repositories::servable(&world.root.join("kept.git")).unwrap_err();
    assert!(!error.is_empty());
}
