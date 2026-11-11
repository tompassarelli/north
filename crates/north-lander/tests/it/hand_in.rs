use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Output};

use north_base::foundation::shim::write_executable;
use north_base::platform::wait;
use north_lander::land::{enqueue, hand_in, queue};

use super::land_first::{NO_SECRETS, PUBLIC_REPO, queueing};
use super::serve::{HANG, Repository, World, git, report};

const VERB: &str = "hand_in::a_hand_in_verb_call_lands_named_paths_staged_now_or_committed_first";
const VERB_ARGUMENTS: &str = "NORTH_HAND_IN_IT_ARGUMENTS";
const ROAD: &str = "serve::enqueue_process";
const ROAD_ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
const JUDGE_PASSES: &str = "#!/bin/sh\ncat > /dev/null\nexit 0\n";
const JUDGE_FINDS_AN_ADDRESS: &str = "#!/bin/sh\ntext=$(cat)\ncase \"$text\" in\n  *'lives at'*) printf 'address\\tthe owner lives at 1 Example Road\\n'; exit 1 ;;\nesac\nexit 0\n";

fn scanned_world(judge: &str) -> (World, Repository) {
    let world = World::new();
    let north = world.repository("north");
    let bin = world.path().join("bin");
    write_executable(&bin.join("judge"), judge);
    write_executable(&bin.join("betterleaks"), NO_SECRETS);
    write_executable(&bin.join("gh"), PUBLIC_REPO);
    fs::write(world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    fs::write(world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();
    (world, north)
}

fn worktree(north: &Repository, branch: &str) -> PathBuf {
    let lane = north.checkout.parent().unwrap().join("worktrees").join(branch);
    git(&north.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), "origin/main"]);
    lane
}

fn verb(world: &World, message: &str, paths: &[&Path]) -> Child {
    let arguments: Vec<&str> = ["--message", message, "--"].into_iter().chain(paths.iter().map(|path| path.to_str().unwrap())).collect();
    queueing(world, VERB, VERB_ARGUMENTS, &arguments).spawn().unwrap()
}

fn safe_push_road(world: &World, lane: &Path) -> Child {
    queueing(world, ROAD, ROAD_ARGUMENTS, &["--repo", lane.to_str().unwrap(), "--wait"]).spawn().unwrap()
}

fn lander(world: &World, north: &Repository) -> Output {
    let bin = world.path().join("bin");
    let mut lander = world.land(&["serve", "--once", "--repo", north.checkout.to_str().unwrap(), "--checker", bin.join("checker").to_str().unwrap(), "--app-token", bin.join("token").to_str().unwrap()]);
    lander
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .env("NORTH_BIN", bin.join("judge"))
        .env("SAFE_PUSH_DENYLIST", world.path().join("denylist.toml"))
        .env("NORTH_PRIVATE_KINDS", world.path().join("private-kinds.toml"))
        .env_remove("SAFE_PUSH_SOUTH");
    lander.output().unwrap()
}

/// Waits for `hand` to queue its lane, serves the queue once and returns what `hand` printed once the lander decided.
fn decided(world: &World, north: &Repository, mut hand: Child) -> (Output, String) {
    let queued = || queue::pending(&north.queue).is_ok_and(|pending| !pending.is_empty());
    assert!(wait::until(HANG, || queued() || hand.try_wait().unwrap().is_some()), "the hand-in neither queued nor exited");
    let journal = if queued() { report(&lander(world, north)) } else { String::new() };
    (hand.wait_with_output().unwrap(), journal)
}

fn refusals(output: &Output) -> Vec<String> {
    [&output.stdout, &output.stderr].into_iter().flat_map(|stream| String::from_utf8_lossy(stream).lines().filter(|line| line.contains('✗')).map(str::to_owned).collect::<Vec<_>>()).collect()
}

#[test]
fn a_hand_in_verb_call_lands_named_paths_staged_now_or_committed_first() {
    if let Ok(arguments) = std::env::var(VERB_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--wait".to_owned()]);
        std::process::exit(i32::from(hand_in::run(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    let lane = worktree(&north, "verb");
    fs::write(lane.join("named.txt"), "the named change\n").unwrap();
    fs::write(lane.join("unnamed.txt"), "a change the worker did not name\n").unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    for (paths, refusal) in [([lane.join("named.txt"), north.checkout.join("README")], "is outside the lane"), ([lane.join("README"), lane.join("README")], "no changes in")] {
        let output = verb(&world, "Add the named note", &[&paths[0], &paths[1]]).wait_with_output().unwrap();
        let text = report(&output);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains(refusal) && text.contains("NOT committed"), "{text}");
        assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");
        assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused hand-in queued: {text}");
    }

    let (output, journal) = decided(&world, &north, verb(&world, "Add the named note (north#1158)", &[&lane.join("named.txt")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "the verb did not return the lander's verdict: {text}\n{journal}");
    assert_eq!(git(&north.origin, &["log", "-1", "--format=%s", "main"]), "Add the named note (north#1158)");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]), "named.txt", "the verb committed more than the named path");

    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "committed");
    fs::write(lane.join("committed.txt"), "the committed change\n").unwrap();
    git(&lane, &["add", "committed.txt"]);
    git(&lane, &["commit", "-qm", "Commit the change first"]);
    fs::write(lane.join("other.txt"), "a change committed beside it\n").unwrap();
    git(&lane, &["add", "other.txt"]);
    git(&lane, &["commit", "-qm", "Commit another change"]);
    let start = git(&lane, &["rev-parse", "HEAD"]);
    let base = git(&north.origin, &["rev-parse", "main"]);

    let refused = verb(&world, "Add the committed note", &[&lane.join("committed.txt")]).wait_with_output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("other.txt") && text.contains("NOT committed"), "{text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused hand-in queued: {text}");

    let (output, journal) = decided(&world, &north, verb(&world, "Add the committed note (north#1158)", &[&lane.join("committed.txt"), &lane.join("other.txt")]));
    let text = report(&output);
    let committed_landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {committed_landed} on main")), "the verb did not queue the committed lane: {text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff", "--name-only", &base, &committed_landed]), "committed.txt\nother.txt", "the lander did not land exactly the committed paths");
}

fn refused_by_both_roads(world: &World, north: &Repository, lane: &Path, path: &Path, refusal: &str) {
    let by_verb = verb(world, "Hand in the refused change", &[path]).wait_with_output().unwrap();
    let by_safe_push = safe_push_road(world, lane).wait_with_output().unwrap();
    assert_eq!((by_verb.status.code(), by_safe_push.status.code()), (Some(1), Some(1)), "{}\n{}", report(&by_verb), report(&by_safe_push));
    assert!(refusals(&by_verb).iter().any(|line| line.contains(refusal)), "{}", report(&by_verb));
    assert_eq!(refusals(&by_verb), refusals(&by_safe_push), "a gate refused the lane in other words through the verb");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused lane was queued");
}

#[test]
fn a_hand_in_verb_call_returns_the_same_refusal_as_safe_push_for_a_refused_lane() {
    let (world, north) = scanned_world(JUDGE_FINDS_AN_ADDRESS);
    let lane = worktree(&north, "conflict");
    fs::write(north.checkout.join("README"), "main moved on\n").unwrap();
    git(&north.checkout, &["commit", "-qam", "Main rewrites the README"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    fs::write(lane.join("README"), "the lane rewrites it too\n").unwrap();
    refused_by_both_roads(&world, &north, &lane, &lane.join("README"), "conflicts with origin/main");

    let lane = worktree(&north, "private");
    fs::write(lane.join("private.txt"), "the owner lives at 1 Example Road\n").unwrap();
    refused_by_both_roads(&world, &north, &lane, &lane.join("private.txt"), "PRIVATE KIND");
}
