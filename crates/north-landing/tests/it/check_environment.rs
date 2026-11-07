use std::fs;

use north_base::foundation::shim::write_executable;
use north_base::foundation::offload::LOCAL_ENV;
use north_landing::land::queue;

use super::serve::{World, git, report};

fn checker() -> String {
    format!("#!/bin/sh\nif [ \"${LOCAL_ENV}\" != 1 ]; then echo \"safe-push: verdict transient main's check environment is broken: a 1 s deadline test failed on gex45\"; exit 0; fi\necho 'safe-push: verdict green'\n")
}

fn verdict_of(queue: &std::path::Path, branch: &str) -> Option<String> {
    fs::read_dir(queue.join("done")).into_iter().flatten().flatten().filter_map(|entry| queue::read(&entry.path()).ok()).find(|ticket| ticket.branch == branch).and_then(|ticket| ticket.verdict)
}

// KEEP(bug): on 2026-11-07 main failed a test only in the build hosts' check environment and the lander paused every landing for over 15 minutes (north#782)
#[test]
fn a_base_red_only_in_the_placed_environment_lands_the_lane_through_this_hosts_environment_and_later_lanes_check_there_first() {
    let world = World::new();
    write_executable(&world.path().join("bin/checker"), checker());
    let repository = world.repository("north");

    repository.lane("first");
    let output = world.lander(&repository, None).wait_with_output().unwrap();
    let transcript = report(&output);
    assert!(verdict_of(&repository.queue, "first").is_some_and(|verdict| verdict.starts_with("landed")), "{transcript}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("judging the candidate in local instead"), "{transcript}");
    assert!(!repository.queue.join("base-broken").exists(), "{transcript}");

    repository.lane("second");
    let output = world.lander(&repository, None).wait_with_output().unwrap();
    let transcript = report(&output);
    assert!(verdict_of(&repository.queue, "second").is_some_and(|verdict| verdict.starts_with("landed")), "{transcript}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("judging the candidate in"), "{transcript}");
    assert_eq!(git(&repository.origin, &["log", "--format=%s", "main"]).lines().take(2).collect::<Vec<_>>(), ["second (north#360)", "first (north#360)"], "{transcript}");
}
