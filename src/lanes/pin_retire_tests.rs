use std::fs;
use std::path::{Path, PathBuf};

use super::pin_retire::{ProofError, consumer_proof, execute};
use super::test_git::git_in;

struct World {
    root: tempfile::TempDir,
}

impl World {
    fn root(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    fn container(&self) -> PathBuf {
        self.root().join("code/proj")
    }

    fn main(&self) -> PathBuf {
        self.container().join("main")
    }

    fn consumer(&self) -> PathBuf {
        self.root().join("code/consumer/main")
    }

    fn remote(&self, name: &str) -> PathBuf {
        self.root().join("remotes").join(format!("{name}.git"))
    }

    fn retire(&self, arguments: &[&str]) -> Result<Vec<String>, String> {
        execute(&self.root(), &arguments.iter().map(|argument| (*argument).to_string()).collect::<Vec<_>>())
    }

    fn new_pin(&self, label: &str) -> PathBuf {
        let history = self.main().join("history.txt");
        let mut text = fs::read_to_string(&history).unwrap_or_default();
        text.push_str(&format!("{label}\n"));
        fs::write(&history, text).unwrap();
        git_in(&self.main(), &["add", "history.txt"]);
        git_in(&self.main(), &["commit", "-qm", label]);
        let oid = git_in(&self.main(), &["rev-parse", "HEAD"]);
        let pin = self.container().join("pins").join(&oid);
        git_in(&self.main(), &["worktree", "add", "-q", "--detach", &pin.to_string_lossy(), &oid]);
        fs::write(sidecar(&pin), format!("consumer-main: {}\nConsumer: fixture {label}.\n", self.consumer().display())).unwrap();
        pin
    }

    fn untouched(&self, pin: &Path) -> bool {
        pin.is_dir() && sidecar(pin).is_file()
    }
}

fn sidecar(pin: &Path) -> PathBuf {
    PathBuf::from(format!("{}.pin", pin.display()))
}

fn bare(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git_in(path, &["init", "-q", "--bare"]);
}

impl World {
    fn new() -> Self {
        let world = Self { root: tempfile::tempdir().unwrap() };
        fs::create_dir_all(world.container().join("pins")).unwrap();
        let main = world.main();
        fs::create_dir_all(&main).unwrap();
        git_in(&main, &["init", "-q", "-b", "main"]);
        bare(&world.remote("proj"));
        git_in(&main, &["remote", "add", "origin", &world.remote("proj").to_string_lossy()]);
        let consumer = world.consumer();
        bare(&world.remote("consumer"));
        fs::create_dir_all(&consumer).unwrap();
        git_in(&consumer, &["init", "-q", "-b", "main"]);
        fs::write(consumer.join("pin.ref"), "current\n").unwrap();
        git_in(&consumer, &["add", "pin.ref"]);
        git_in(&consumer, &["commit", "-qm", "current"]);
        git_in(&consumer, &["remote", "add", "origin", &world.remote("consumer").to_string_lossy()]);
        git_in(&consumer, &["push", "-qu", "origin", "main"]);
        world
    }

    fn publish_consumer(&self, name: &str, content: &str) {
        let consumer = self.consumer();
        fs::write(consumer.join(name), content).unwrap();
        git_in(&consumer, &["add", name]);
        git_in(&consumer, &["commit", "-qm", name]);
        git_in(&consumer, &["push", "-qu", "origin", "main"]);
    }
}

fn arguments<'a>(consumer: &'a str, pin: &'a str) -> [&'a str; 4] {
    ["--consumer-main", consumer, "--", pin]
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn a_pin_consumed_by_its_own_repository_is_retired_once_no_other_lane_references_it() {
    let world = World::new();
    let pin = world.new_pin("same-repository-consumer");
    git_in(&world.main(), &["push", "-qu", "origin", "main"]);
    fs::write(sidecar(&pin), format!("consumer-main: {}\nConsumer: same repository fixture.\n", world.main().display())).unwrap();
    let main = text(&world.main());

    world.retire(&["--dry-run", "--consumer-main", &main, "--", &text(&pin)]).expect("a pin's own administrative pointer is not a consumer");

    let lane = world.container().join("worktrees/other");
    git_in(&world.main(), &["worktree", "add", "-q", "-b", "same-repository-reference", &text(&lane), "main"]);
    fs::write(lane.join("pin.ref"), format!("pin {}\n", pin.display())).unwrap();
    git_in(&lane, &["add", "pin.ref"]);
    git_in(&lane, &["commit", "-qm", "other lane uses same-repository pin"]);
    let refused = world.retire(&["--dry-run", "--consumer-main", &main, "--", &text(&pin)]).unwrap_err();
    assert!(refused.contains(&lane.to_string_lossy().into_owned()), "{refused}");

    git_in(&world.main(), &["worktree", "remove", &text(&lane)]);
    world.retire(&["--consumer-main", &main, "--", &text(&pin)]).unwrap();
    assert!(!pin.exists() && !sidecar(&pin).exists());
}

#[test]
fn a_moved_consumer_retires_the_pin_its_sidecar_and_its_registration() {
    let world = World::new();
    let pin = world.new_pin("valid");
    let lines = world.retire(&arguments(&text(&world.consumer()), &text(&pin))).unwrap();
    assert_eq!(lines, [format!("retired pin {}", pin.display())]);
    assert!(!pin.exists() && !sidecar(&pin).exists());
    assert!(!git_in(&world.main(), &["worktree", "list", "--porcelain"]).contains(&text(&pin)));
}

#[test]
fn every_unproven_consumer_state_leaves_the_pin_and_its_sidecar_untouched() {
    let world = World::new();
    let pin = world.new_pin("proof-required");
    let consumer = text(&world.consumer());
    let pin_arg = text(&pin);

    assert!(world.retire(&["--", &pin_arg]).is_err(), "missing consumer proof");
    assert!(world.untouched(&pin));

    let other = world.root().join("code/other/main");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    git_in(&world.root(), &["clone", "-qb", "main", &text(&world.remote("consumer")), &text(&other)]);
    git_in(&other, &["checkout", "-q", "--detach"]);
    assert!(world.retire(&arguments(&text(&other), &pin_arg)).is_err(), "detached consumer checkout");
    git_in(&other, &["switch", "-q", "main"]);
    let mismatch = world.retire(&arguments(&text(&other), &pin_arg)).unwrap_err();
    assert!(mismatch.contains("do not exactly match the sidecar"), "{mismatch}");
    assert!(world.untouched(&pin));

    fs::write(world.consumer().join("ahead.txt"), "ahead\n").unwrap();
    git_in(&world.consumer(), &["add", "ahead.txt"]);
    git_in(&world.consumer(), &["commit", "-qm", "ahead"]);
    let unpublished = world.retire(&arguments(&consumer, &pin_arg)).unwrap_err();
    assert!(unpublished.contains("does not match origin/main"), "{unpublished}");
    assert!(world.untouched(&pin));
    git_in(&world.consumer(), &["push", "-qu", "origin", "main"]);

    let lane = world.root().join("code/consumer/worktrees/playable");
    git_in(&world.consumer(), &["worktree", "add", "-q", "-b", "playable", &text(&lane), "main"]);
    fs::write(lane.join("pin.ref"), format!("pin {pin_arg}\n")).unwrap();
    git_in(&lane, &["add", "pin.ref"]);
    git_in(&lane, &["commit", "-qm", "playable lane uses pin"]);
    let lane_error = world.retire(&arguments(&consumer, &pin_arg)).unwrap_err();
    assert!(lane_error.contains(&text(&lane)), "{lane_error}");
    git_in(&world.consumer(), &["worktree", "remove", &text(&lane)]);

    let dry = world.retire(&["--dry-run", "--consumer-main", &consumer, "--", &pin_arg]).unwrap();
    assert!(dry[0].contains("DRY RUN") && dry[1].contains(&pin_arg), "{dry:?}");
    assert!(world.untouched(&pin));
}

#[test]
fn a_dirty_pin_an_empty_sidecar_and_a_misnamed_pin_are_refused() {
    let world = World::new();
    let pin = world.new_pin("dirty");
    let consumer = text(&world.consumer());
    let pin_arg = text(&pin);

    fs::write(pin.join("untracked.txt"), "untracked\n").unwrap();
    let dirty = world.retire(&arguments(&consumer, &pin_arg)).unwrap_err();
    assert!(dirty.contains("tracked or untracked changes"), "{dirty}");
    assert!(world.untouched(&pin));
    fs::remove_file(pin.join("untracked.txt")).unwrap();

    fs::write(sidecar(&pin), "").unwrap();
    assert!(world.retire(&arguments(&consumer, &pin_arg)).is_err(), "empty sidecar");
    assert!(world.untouched(&pin));

    fs::write(sidecar(&pin), format!("consumer-main: {consumer}\nConsumer: fixture.\n")).unwrap();
    let wrong = world.container().join("pins/0000000000000000000000000000000000000000");
    git_in(&world.main(), &["worktree", "move", &pin_arg, &text(&wrong)]);
    fs::rename(sidecar(&pin), sidecar(&wrong)).unwrap();
    assert!(world.retire(&arguments(&consumer, &text(&wrong))).is_err(), "path and HEAD mismatch");
    assert!(world.untouched(&wrong));
    assert!(world.retire(&arguments(&consumer, &text(&world.container().join("pins")))).is_err(), "pins root");

    git_in(&world.main(), &["worktree", "move", &text(&wrong), &pin_arg]);
    fs::rename(sidecar(&wrong), sidecar(&pin)).unwrap();
    world.publish_consumer("pin.ref", &format!("pin {pin_arg}\n"));
    let live = world.retire(&arguments(&consumer, &pin_arg)).unwrap_err();
    assert!(live.contains("still references the pin"), "{live}");
    assert!(world.untouched(&pin));

    fs::write(sidecar(&pin), "Consumer: unstructured fixture.\n").unwrap();
    assert!(world.retire(&arguments(&consumer, &pin_arg)).is_err(), "unstructured legacy sidecar");
    assert!(world.untouched(&pin));
}

#[test]
fn an_unconsumed_pin_retires_only_when_reachable_unreferenced_and_clean() {
    let world = World::new();
    let referenced = world.new_pin("unconsumed-referenced");
    fs::remove_file(sidecar(&referenced)).unwrap();
    git_in(&world.main(), &["push", "-qu", "origin", "main"]);
    let reference = world.consumer().join("unconsumed.ref");
    fs::write(&reference, format!("pin {}\n", referenced.file_name().unwrap().to_string_lossy())).unwrap();
    let error = world.retire(&["--unconsumed", "--", &text(&referenced)]).unwrap_err();
    assert!(error.contains(&text(&reference)), "{error}");
    assert!(referenced.is_dir());
    fs::remove_file(&reference).unwrap();

    let lost = git_in(&world.main(), &["commit-tree", "-m", "unreachable", "HEAD^{tree}"]);
    let lost_pin = world.container().join("pins").join(&lost);
    git_in(&world.main(), &["worktree", "add", "-q", "--detach", &text(&lost_pin), &lost]);
    assert!(world.retire(&["--unconsumed", "--", &text(&lost_pin)]).is_err(), "unreachable HEAD");
    assert!(lost_pin.is_dir());

    let proof = world.retire(&["--unconsumed", "--", &text(&referenced)]).unwrap();
    assert!(proof[0].contains("--unconsumed proof") && proof.last().unwrap().starts_with("retired pin"), "{proof:?}");
    assert!(!referenced.exists());
}

#[test]
fn a_short_named_unconsumed_pin_is_held_by_its_path_and_released_by_a_cited_full_commit() {
    let world = World::new();
    let seed = world.new_pin("seed");
    fs::remove_file(sidecar(&seed)).unwrap();
    fs::write(world.main().join("history.txt"), "short\n").unwrap();
    git_in(&world.main(), &["commit", "-qam", "short-named"]);
    git_in(&world.main(), &["push", "-q", "origin", "main"]);
    let oid = git_in(&world.main(), &["rev-parse", "HEAD"]);
    let pin = world.container().join("pins").join(&oid[..12]);
    git_in(&world.main(), &["worktree", "add", "-q", "--detach", &text(&pin), &oid]);
    let reference = world.consumer().join("short.ref");
    fs::write(&reference, format!("see ~/code/proj/pins/{}\n", &oid[..12])).unwrap();
    assert!(world.retire(&["--unconsumed", "--", &text(&pin)]).is_err(), "short name cited by its pin path");
    fs::write(&reference, format!("fixed in commit {oid}\n")).unwrap();
    world.retire(&["--unconsumed", "--", &text(&pin)]).expect("a full commit citation is not a pin reference");
    assert!(!pin.exists());
}

#[test]
fn the_consumer_proof_is_a_set_comparison_that_names_each_way_it_fails() {
    let mut seed: u64 = 317;
    for trial in 0..64 {
        seed = (1_103_515_245 * seed + 12345) & 0x7fff_ffff;
        let first = format!("/consumer/{trial}/main");
        let second = format!("/consumer/{}/main", trial + 1);
        let supplied = vec![first.clone(), second.clone()];
        let declared = vec![second.clone(), first.clone()];
        assert_eq!(consumer_proof(&supplied, &declared), Ok(()), "permutation, seed {seed}");
        let doubled = [supplied.clone(), vec![first.clone()]].concat();
        assert_eq!(consumer_proof(&doubled, &declared), Err(ProofError::DuplicateArguments));
        let doubled_sidecar = [declared.clone(), vec![first.clone()]].concat();
        assert_eq!(consumer_proof(&supplied, &doubled_sidecar), Err(ProofError::DuplicateSidecar));
        let different = vec![format!("/different/{trial}/main"), second.clone()];
        assert_eq!(consumer_proof(&supplied, &different), Err(ProofError::Mismatch));
        assert_eq!(consumer_proof(&supplied[..1], &declared), Err(ProofError::Mismatch));
    }
}
