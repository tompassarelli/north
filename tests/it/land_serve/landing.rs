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
fn a_lane_failing_a_lane_gate_alone_leaves_the_batch_before_the_candidate_is_checked() {
    let mut fixture = Fixture::new();
    let first = fixture.lane("first", "first.txt", "first\n");
    let skipping = fixture.lane("skipping", "probe_tests.rs", concat!("#[ig", "nore]\nfn probe() {}\n"));
    let last = fixture.lane("last", "last.txt", "last\n");
    let tickets = [first, skipping, last].map(|lane| fixture.enqueue(&lane, None));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.checks().len(), 1, "one candidate, no culprit search");
    assert_eq!(fixture.subjects(), ["change from first", "change from last"]);
    let refused = fixture.verdict(&tickets[1]).unwrap_or_default();
    assert!(refused.starts_with("failed test integrity; log: "), "{refused}");
    let told = fixture.fleet().into_iter().find(|call| call.contains("session-lane-skipping ")).unwrap_or_default();
    assert!(told.contains(concat!("safe-push: ✗ probe_tests.rs adds a skip or focus: #[ig", "nore]")), "{told}");
    for branch in ["lane-first", "lane-last"] {
        let record = fixture.calls("land-train record").into_iter().find(|call| call.contains(&format!("--branch {branch} "))).unwrap_or_default();
        assert!(record.contains("--batch-size 2 --bisect-depth 0 "), "{record}");
    }
}
