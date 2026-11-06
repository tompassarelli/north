use std::fs;
use std::path::Path;

use north_base::foundation::shim::write_executable;
use north_landing::land::queue;

use super::serve::{Repository, World, report};

const CHECKER: &str = r#"#!/bin/sh
if [ -e crate.txt ]; then
  printf 'error: failed to download `aes v0.8.4`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n'
  echo 'safe-push: land check cargo clippy exited 101; log: /dev/null'
  echo 'safe-push: land check failed; rerunning it on base tree 0123456789ab: cargo clippy'
  echo 'safe-push: verdict refused cargo clippy exited 101'
  exit 1
fi
if [ -e flaky.txt ]; then echo 'safe-push: verdict transient the connection dropped'; exit 0; fi
echo 'safe-push: verdict green'
"#;

fn verdicts(queue: &Path) -> Vec<(String, String)> {
    let mut decided: Vec<_> = fs::read_dir(queue.join("done")).into_iter().flatten().flatten().filter_map(|entry| queue::read(&entry.path()).ok()).filter_map(|ticket| Some((ticket.branch, ticket.verdict?))).collect();
    decided.sort();
    decided
}

fn verdict_of(repository: &Repository, branch: &str) -> Option<String> {
    verdicts(&repository.queue).into_iter().find(|(name, _)| name == branch).map(|(_, verdict)| verdict)
}

#[test]
fn a_lane_whose_check_fails_with_a_host_looking_error_main_does_not_reproduce_is_refused_within_one_alone_check_though_the_lanes_around_it_change_each_round() {
    let world = World::new();
    write_executable(&world.path().join("bin/checker"), CHECKER);
    let repository = world.repository("north");

    repository.lane("crate");
    repository.lane("first");
    let output = world.lander(&repository, None).wait_with_output().unwrap();
    let transcript = report(&output);
    let refused = verdict_of(&repository, "crate").unwrap_or_default();
    assert!(refused.starts_with("failed "), "main passed the rerun, so the missing crate is the lane's: {refused}\n{transcript}");
    assert_ne!(verdict_of(&repository, "first").as_deref().map(|verdict| verdict.starts_with("failed ")), Some(true), "{transcript}");

    repository.lane("flaky");
    for round in 0..3 {
        repository.lane(&format!("arriving-{round}"));
        let output = world.lander(&repository, None).wait_with_output().unwrap();
        println!("{}", report(&output));
    }
    let persistent = verdict_of(&repository, "flaky").unwrap_or_default();
    assert!(persistent.starts_with("failed persistent transient"), "a lane in three transient candidates is checked alone, though no two candidates held the same lanes: {persistent}");
    assert!(verdicts(&repository.queue).iter().filter(|(name, _)| name.starts_with("arriving-")).all(|(_, verdict)| !verdict.starts_with("failed ")), "{:?}", verdicts(&repository.queue));
}
