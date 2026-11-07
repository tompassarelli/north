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
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'per-lane landing under the lander'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "a lost lease rebuilds and checks again");
    let race = git(&racer, &["rev-parse", "HEAD"]);
    assert_eq!(checks[1].1, race, "the rebuilt candidate is on the new main");
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.subjects(), ["per-lane landing under the lander", "change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_token_scoped() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the check saw {name}");
    }
    assert!(fixture.read("pushes").contains("token=push-token"), "the push step keeps its token");
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
    assert_eq!(fixture.checks().len(), 1, "one candidate, no culprit search");
    assert_eq!(fixture.subjects(), ["change from first", "change from skipping", "change from last"]);
    let shadowed = std::fs::read_to_string(fixture.state.join(north_landing::land::landing_refusals::LOG)).unwrap_or_default();
    assert!(shadowed.contains("\"gate\":\"test integrity\"") && shadowed.contains("\"at\":\"pick\"") && shadowed.contains("lane-skipping"), "{shadowed}");
    assert!(shadowed.contains(concat!("probe_tests.rs adds a skip or focus: #[ig", "nore]")), "{shadowed}");
}
