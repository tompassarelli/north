use super::{Fixture, executable, git};

#[test]
fn serve_cas_lost() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    std::fs::write(racer.join("race.txt"), "race\n").unwrap();
    git(&racer, &["add", "race.txt"]);
    git(&racer, &["commit", "-qm", "per-lane landing under the lander"]);
    git(&racer, &["push", "-q", "origin", "HEAD:refs/heads/race"]);
    executable(&fixture.origin.join("hooks/pre-receive"), "#!/bin/sh\n[ -e \"$GIT_DIR/raced\" ] && exit 0\n: > \"$GIT_DIR/raced\"\nenv -u GIT_QUARANTINE_PATH git update-ref refs/heads/main refs/heads/race\n");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.read("pushes").lines().count(), 2, "a lost lease rebuilds the candidate and pushes again");
    let race = git(&racer, &["rev-parse", "HEAD"]);
    assert_eq!(git(&fixture.origin, &["rev-parse", "main~1"]), race, "the rebuilt candidate is on the new main");
    assert_eq!(fixture.subjects(), ["per-lane landing under the lander", "change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_token_scoped() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let env = fixture.read("after-env");
    assert!(env.contains("LAND_TRACE="), "the after-landing check never ran");
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the after-landing check saw {name}");
    }
    assert!(fixture.read("pushes").contains("token=none app=app-token"), "the push step pushes with the lander App token and no inherited login");
}


#[test]
fn a_lane_failing_a_lane_gate_lands_in_its_batch_with_the_would_be_refusal_logged_while_landing_refusals_is_shadow() {
    let mut fixture = Fixture::new();
    let first = fixture.lane("first", "first.txt", "first\n");
    let skipping = fixture.lane("skipping", "probe_tests.rs", concat!("#[ig", "nore]\nfn probe() {}\n"));
    let last = fixture.lane("last", "last.txt", "last\n");
    for lane in [first, skipping, last] {
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.read("pushes").lines().count(), 1, "all three lanes land in one push");
    assert_eq!(fixture.subjects(), ["change from first", "change from skipping", "change from last"]);
    let shadowed = std::fs::read_to_string(fixture.state.join(north_landing::land::landing_refusals::LOG)).unwrap_or_default();
    assert!(shadowed.contains("\"gate\":\"test integrity\"") && shadowed.contains("\"at\":\"pick\"") && shadowed.contains("lane-skipping"), "{shadowed}");
    assert!(shadowed.contains(concat!("probe_tests.rs adds a skip or focus: #[ig", "nore]")), "{shadowed}");
}

