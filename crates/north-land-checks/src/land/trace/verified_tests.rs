use super::super::profile::Trace;
use super::super::{AFTER, Category, PHASE, Span, Spans};
use super::after_walls;

fn landing(name: &str, landed: &str, start: f64, end: f64) -> Span {
    Span::new(&format!("ticket-{name}"), name, Category::Landing, start, end).with("landed", landed)
}

fn after(trace: &str, start: f64, end: f64) -> Span {
    Span::new(trace, "post-land checks", Category::Check, start, end).with(PHASE, AFTER)
}

fn whole_file(spans: &Spans, since: f64) -> Vec<(i64, f64)> {
    let all = spans.read();
    let trace = Trace::of(&all);
    trace.landings(since).into_iter().map(|landing| (landing.end as i64, trace.profile(landing).after_wall)).filter(|(_, wall)| *wall > 0.0).collect()
}

#[test]
fn a_read_of_the_bytes_a_spans_file_gained_gives_what_a_read_of_the_whole_file_gives_and_a_replaced_file_starts_over() {
    let directory = tempfile::tempdir().unwrap();
    let spans = Spans::at(directory.path().join("spans.jsonl"));
    let cache = directory.path().join("cache.json");
    let now = 100_000.0;
    spans.append(&[landing("one", "a", 1_000.0, 1_100.0), after("a", 1_100.0, 1_160.0)]).unwrap();
    assert_eq!(after_walls(&spans, &cache, 0.0, now), whole_file(&spans, 0.0));
    spans.append(&[landing("two", "b", 90_000.0, 90_100.0), after("b", 90_100.0, 90_130.0), after("a", 1_200.0, 1_230.0)]).unwrap();
    assert_eq!(after_walls(&spans, &cache, 0.0, now), whole_file(&spans, 0.0));
    std::fs::remove_file(spans.path()).unwrap();
    spans.append(&[landing("three", "c", 95_000.0, 95_100.0), after("c", 95_100.0, 95_110.0)]).unwrap();
    assert_eq!(after_walls(&spans, &cache, 0.0, now), vec![(95_100, 10.0)]);
}
