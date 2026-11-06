use std::path::Path;

use north_landing::land::trace::profile::Trace;
use north_landing::land::trace::report::Metric;
use north_landing::land::trace::{Category, Spans, report};

fn near(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-6
}

#[test]
fn the_profile_splits_a_trace_by_category_and_an_old_slow_backlog_outside_the_last_twenty_landings_does_not_refuse_a_lane() {
    a_fixture_trace_splits_queued_at_to_push_by_category_and_reports_verification_after_the_push();
    an_old_slow_backlog_outside_the_last_twenty_landings_does_not_refuse_a_lane();
    the_projected_wait_reads_the_last_landings_service_time_times_the_queue_ahead_so_an_idle_queue_recovers_from_a_slow_hour();
}

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

fn an_old_slow_backlog_outside_the_last_twenty_landings_does_not_refuse_a_lane() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("spans.jsonl");
    let lines: Vec<String> = (0..45)
        .flat_map(|index| {
            let start = 1000.0 * f64::from(index);
            let verification = if index < 25 { 700.0 } else { 60.0 };
            [
                format!(r#"{{"trace":"t{index}","id":"landing:{index}","name":"lane-{index}","category":"landing","start":{start},"end":{},"attributes":{{"landed":"c{index}"}}}}"#, start + 10.0),
                format!(r#"{{"trace":"c{index}","id":"after:{index}","name":"post-land checks","category":"lander","start":{},"end":{},"attributes":{{"phase":"after"}}}}"#, start + 10.0, start + 10.0 + verification),
            ]
        })
        .collect();
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    let spans = Spans::at(&path).read();
    let trace = Trace::of(&spans);
    let median = |landings: Vec<_>| Metric::parse("verification-p50").unwrap().value(&landings.into_iter().map(|landing| trace.profile(landing)).collect::<Vec<_>>());
    let ceiling = 300.0;
    assert!(median(trace.latest(0.0, None)) > ceiling, "the whole day reads the backlog");
    assert_eq!(trace.latest(0.0, Some(20)).len(), 20);
    assert!(near(median(trace.latest(0.0, Some(20))), 60.0), "the last twenty landings read only the recent runs");
}

fn the_projected_wait_reads_the_last_landings_service_time_times_the_queue_ahead_so_an_idle_queue_recovers_from_a_slow_hour() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("spans.jsonl");
    let lines: Vec<String> = (0..30)
        .flat_map(|index| {
            let start = 1000.0 * f64::from(index);
            let (queued, served) = if index < 25 { (120.0, 80.0) } else { (4.0, 12.0) };
            [
                format!(r#"{{"trace":"t{index}","id":"landing:{index}","name":"lane-{index}","category":"landing","start":{start},"end":{},"attributes":{{"landed":"c{index}"}}}}"#, start + queued + served),
                format!(r#"{{"trace":"t{index}","id":"queued:{index}","parent":"landing:{index}","name":"queued","category":"queue","start":{start},"end":{}}}"#, start + queued),
            ]
        })
        .collect();
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    let spans = Spans::at(&path).read();
    let trace = Trace::of(&spans);
    let read = |metric: &str, last: usize, pending: usize| {
        let profiles: Vec<_> = trace.latest(0.0, Some(last)).into_iter().map(|landing| trace.profile(landing)).collect();
        Metric::parse(metric).unwrap().value_behind(&profiles, pending)
    };
    let ceiling = 60.0;
    assert!(read("landing-p50", 20, 0) > ceiling, "the median of the last twenty landings still reads the slow hour");
    assert!(near(read("landing-projected", 5, 0), 12.0) && near(read("landing-projected", 5, 1), 12.0), "an idle queue, or one holding only the judged lane's own ticket, costs one landing's service time");
    assert!(read("landing-projected", 5, 0) <= ceiling, "so the gate admits any lane");
    assert!(near(read("landing-projected", 5, 4), 24.0) && near(read("landing-projected", 5, 40), 24.0), "a burst of queued lanes costs two services, the candidate in flight and the one that carries them all");
    assert!(read("landing-projected", 30, 2) > ceiling, "a service time that breaks the contract still refuses a lane that pays nothing");
    assert!(near(read("landing-projected", 30, 0), 80.0), "a window that keeps the slow hour still reads it");
    assert!(near(read("landing-projected", 5, 0), Metric::parse("landing-projected").unwrap().value(&trace.latest(0.0, Some(5)).into_iter().map(|landing| trace.profile(landing)).collect::<Vec<_>>())), "value reads an empty queue");
}
