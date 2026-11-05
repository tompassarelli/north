use std::path::Path;

use north_landing::land::trace::profile::Trace;
use north_landing::land::trace::{Category, Spans, report};

fn near(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-6
}

#[test]
fn a_fixture_trace_splits_queued_at_to_push_by_category_and_reports_verification_after_the_push() {
    let spans = Spans::at(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/fixtures/land_profile_spans.jsonl")).read();
    assert_eq!(spans.len(), 20);
    let trace = Trace::of(&spans);
    let landings = trace.landings(0.0);
    assert_eq!(landings.len(), 1);
    let profile = trace.profile(landings[0]);
    assert!(near(profile.before.total(), profile.wall()), "{:?}", profile.before);
    let expected = [
        (Category::Queue, 310.0),
        (Category::Lander, 30.0),
        (Category::HostWait, 10.0),
        (Category::Lint, 29.75),
        (Category::Docs, 5.25),
        (Category::Offload, 15.0),
        (Category::Compile, 80.0),
        (Category::Test, 180.0),
        (Category::Git, 40.0),
    ];
    for (category, seconds) in expected {
        assert!(near(profile.before.get(category), seconds), "{category:?}: {:?}", profile.before);
    }
    assert!(near(profile.before.crates["north"], 100.0) && near(profile.before.crates["it (tests/it/main.rs)"], 70.0), "{:?}", profile.before.crates);
    assert!(near(profile.after_wall, 200.0));
    for (category, seconds) in [(Category::Lander, 15.0), (Category::Ci, 10.0), (Category::Compile, 85.0), (Category::Test, 90.0)] {
        assert!(near(profile.after.get(category), seconds), "{category:?}: {:?}", profile.after);
    }

    let text = report::render(&[profile], 0.0);
    for line in [
        "north land profile: 1 landing since ",
        "queued_at to push: p50 700 s, p90 700 s, sum 700 s",
        "wall 700 s, categories sum 700 s (+0.0%)",
        "  queue wait 310 s 44%, host wait 10 s 1%, sync/offload 15 s 2%, compile 80 s 11%, test run 180 s 26% (north 100, it (tests/it/main.rs) 70), lint 30 s 4%, docs/index checks 5 s 1%, git rebase/push 40 s 6%, safe-push and lander 30 s 4%",
        "  after push, 200 s of verification: compile 85 s 42%, test run 90 s 45% (north 80), safe-push and lander 15 s 8%, CI 10 s 5%",
        "- queue wait                310 s   44%  p50   310 s  p90   310 s  waiting",
        "  test run by crate: north 100 s, it (tests/it/main.rs) 70 s",
        "1. 300 s queue wait: queued (lane-a)",
        "2. 100 s test run: test north on gex45 (lane-a)",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
    assert_eq!(text.lines().filter(|line| line.starts_with(|first: char| first.is_ascii_digit()) && line.contains(". ")).count(), 10, "{text}");
}
