use std::fs;

use super::{Fixture, Landing, Run, Scratch, git};

#[test]
fn check_candidate_ends_verdict_red_main_for_a_red_main_failure_and_verdict_green_for_a_green_one_and_pushes_nothing() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "check-candidate");
    let lane = landing.lane();
    landing.configure(&lane, &[("land.workflow", "land-check.yml")], &["true"], "a land check on Actions");
    let base = landing.main();
    let candidate = |extra: &[(&str, &str)]| {
        let mut run = Run::new(&scratch, &lane, &["--check-candidate", "--base", &base]).env("SAFE_PUSH_TEST_RED_TRACE", scratch.path().join("red-trace"));
        for (name, value) in extra {
            run = run.env(name, value);
        }
        run.output()
    };

    let red = candidate(&[("SAFE_PUSH_TEST_ACTIONS_STATUS", "1"), ("SAFE_PUSH_TEST_RED_CHECK_STATUS", "3")]);
    assert_ne!(red.status, Some(0), "a red-main candidate exited 0: {}", red.output);
    assert_eq!(red.last_line(), format!("safe-push: verdict red-main {base}"), "{}", red.output);
    assert!(std::fs::read_to_string(scratch.path().join("red-trace")).unwrap_or_default().contains("land-red-check"), "red main was not asked for: {}", red.output);

    let green = candidate(&[]);
    assert_eq!(green.status, Some(0), "a green candidate failed: {}", green.output);
    assert_eq!(green.last_line(), format!("safe-push: verdict green {}", git(&lane, &["rev-parse", "HEAD"])), "{}", green.output);
    assert_eq!(landing.main(), base, "check-candidate pushed");
}

#[test]
fn check_candidate_names_the_broken_base_transient_when_main_fails_its_own_check_though_a_queued_admission_line_names_defer_memory_capacity() {
    let scratch = Scratch::new();
    let landing = Landing::new(&scratch, "check-candidate-broken-main");
    landing.configure(&landing.seed, &[], &["echo '{\"decision\":\"QUEUED\",\"reason\":\"DEFER_MEMORY_CAPACITY\"}'; echo 'error[E0063]: missing field'; exit 101"], "a land check that fails on every tree after a queued admission");
    git(&landing.seed, &["push", "-q", &landing.remote.to_string_lossy(), "main"]);
    let lane = landing.lane();
    let base = landing.main();
    let observed = Run::new(&scratch, &lane, &["--check-candidate", "--base", &base]).env("NORTH_LAND_ROUND", "582").output();
    assert_ne!(observed.status, Some(0), "a candidate on a main that fails its own check exited 0: {}", observed.output);
    assert!(observed.last_line().starts_with("safe-push: verdict transient main's check environment is broken: "), "{}", observed.output);
}

#[test]
fn land_publish_pushes_the_scanned_lane_to_a_scratch_ref_for_the_land_checks_and_deletes_it_afterwards() {
    let fixture = Fixture::new();
    let (published_ref, published_head) = (fixture.root.join("published-ref"), fixture.root.join("published-head"));
    fixture.git(&["config", "--file", ".safe-push", "land.publish", "farm/"]);
    let check = format!(
        "file.txt :: printf '%s\\n' \"$SAFE_PUSH_LAND_REF\" > '{}'; git --git-dir='{}' rev-parse \"refs/heads/$SAFE_PUSH_LAND_REF\" > '{}'",
        published_ref.display(),
        fixture.remote.display(),
        published_head.display()
    );
    fs::write(fixture.repo.join("file.txt"), "published\n").unwrap();
    fixture.git(&["add", "file.txt"]);
    fixture.land_checks(&[&check], "publish for the land check");
    let (base, head) = (fixture.git(&["rev-parse", "origin/main"]), fixture.head());
    fixture.run(&["--check-candidate", "--base", &base]).expect(true, &["safe-push: verdict green"]);
    let scratch_ref = format!("farm/{}", &head[..12]);
    assert_eq!(fs::read_to_string(&published_ref).unwrap().trim(), scratch_ref, "the land check did not get the published ref");
    assert_eq!(fs::read_to_string(&published_head).unwrap().trim(), head, "the published ref did not hold the lane head while the land check ran");
    assert!(!git(&fixture.remote, &["for-each-ref", "--format=%(refname)"]).contains(&scratch_ref), "the published ref outlived the land checks");
}
