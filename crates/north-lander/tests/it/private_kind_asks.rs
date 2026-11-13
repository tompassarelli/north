use std::fs;

use super::land_first::{NO_SECRETS, PUBLIC_REPO, Release, calls, checker, enqueue};
use super::serve::{HANG, World, git, report};

const JUDGE_HOLDS_ONE_LANE: &str = "#!/bin/sh\ntext=$(cat)\ncase \"$text\" in\n  *held*) echo >> \"$0.held\"; while [ ! -e \"$HOME/judge-release\" ]; do sleep 0.05; done ;;\n  *) while [ ! -e \"$HOME/judge-others\" ]; do sleep 0.05; done ;;\nesac\nexit 0\n";

#[test]
fn three_lanes_handed_in_together_land_while_one_of_them_still_waits_for_its_private_kind_verdict_north_1199() {
    let world = World::new();
    let north = world.repository("north");
    let bin = world.path().join("bin");
    north_base::foundation::shim::write_executable(&bin.join("judge"), JUDGE_HOLDS_ONE_LANE);
    north_base::foundation::shim::write_executable(&bin.join("betterleaks"), NO_SECRETS);
    north_base::foundation::shim::write_executable(&bin.join("gh"), PUBLIC_REPO);
    fs::write(world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    fs::write(world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();
    checker(&world);
    for branch in ["held", "second", "third"] {
        let lane = north.checkout.parent().unwrap().join("worktrees").join(branch);
        git(&north.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), "origin/main"]);
        fs::write(lane.join(format!("{branch}.txt")), "a public note\n").unwrap();
        git(&lane, &["add", "-A"]);
        git(&lane, &["commit", "-q", "-m", &format!("Add the {branch} note")]);
        let output = enqueue(&world, &lane);
        let text = report(&output);
        assert!(text.contains("the private-kind judge is still judging this lane's text"), "{branch} was not queued before its private-kind verdict: {text}");
    }
    let enqueue_asks = calls(&bin.join("judge.held"));
    fs::write(world.path().join("judge-others"), "").unwrap();

    let release = Release(world.path().join("judge-release"));
    let mut command = world.land(&["serve", "--repo", north.checkout.to_str().unwrap(), "--checker", bin.join("checker").to_str().unwrap(), "--app-token", bin.join("token").to_str().unwrap()]);
    command
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .env("NORTH_BIN", bin.join("judge"))
        .env("SAFE_PUSH_DENYLIST", world.path().join("denylist.toml"))
        .env("NORTH_PRIVATE_KINDS", world.path().join("private-kinds.toml"))
        .env_remove("SAFE_PUSH_SOUTH");
    let mut lander = command.spawn().unwrap();
    let landed = |file: &str| git(&north.origin, &["ls-tree", "--name-only", "main"]).lines().any(|name| name == file);
    north_base::platform::wait::until(HANG, || landed("second.txt") && landed("third.txt") || lander.try_wait().unwrap().is_some());
    let others = landed("second.txt") && landed("third.txt");
    let held_asks = calls(&bin.join("judge.held"));
    let held_early = landed("held.txt");
    drop(release);
    north_base::platform::wait::until(HANG, || landed("held.txt") || lander.try_wait().unwrap().is_some());
    let held_landed = landed("held.txt");
    let _ = lander.kill();
    let journal = report(&lander.wait_with_output().unwrap());
    assert!(others, "the second and third lanes never landed: {journal}");
    assert!(!held_early, "a lane was pushed before its private-kind verdict: {journal}");
    assert_eq!(held_asks, enqueue_asks + 1, "the second and third lanes landed only after the lander gave up on the held lane's verdict ask and asked again, so their push waited on another lane's verdict: {journal}");
    assert!(held_landed, "the held lane did not land once its judge passed it: {journal}");
}
