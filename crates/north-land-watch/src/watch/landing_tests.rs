use super::*;
use crate::watch::facts::StageSample;

const NOW: u64 = 1_800_000_000;

fn context(p90_seconds: f64, running: f64) -> WatchContext {
    let sample = |seconds| StageSample { repo: "north".into(), stage: "land checks".into(), seconds, total: seconds };
    let waiting = Event { time: NOW as f64 - running, repo: "north".into(), lane: "lane-a,lane-b".into(), stage: "land checks".into(), status: "wait".into(), pid: 7, ..Event::default() };
    WatchContext { now: NOW, events: vec![waiting], stages: (0..10).map(|_| sample(p90_seconds)).collect(), live: BTreeSet::from([7]), ..WatchContext::default() }
}

#[test]
fn a_stage_past_its_p90_raises_the_overrun_alarm_only_past_the_floor_and_names_its_lane() {
    assert!(stalls(&context(3.0, 21.0)).is_empty(), "21 s against a 3 s p90 is under the floor");
    let past_both = stalls(&context(3.0, 700.0));
    assert_eq!(past_both.len(), 1);
    assert!(past_both[0].summary.contains("lane lane-a,lane-b:"), "{}", past_both[0].summary);
    assert!(stalls(&context(500.0, 700.0)).is_empty(), "700 s is under 1.5 times a 500 s p90");
}

#[test]
fn two_hosts_seeing_one_github_event_alarm_only_from_the_lander_host() {
    use crate::watch::facts::{RepoFacts, defers_to};
    let seen_by = |here: &str| {
        let repo = RepoFacts { name: "wisp".into(), error: Some("gh api: HTTP 502".into()), ..RepoFacts::default() };
        let settle = serde_json::json!({"landing": "abc", "checkedAt": NOW, "hosts": [{"host": "gex45", "problems": ["not live"]}]});
        let context = WatchContext { now: NOW, repos: vec![repo], settle: Some(settle), defers_fleet_alarms: defers_to(Some("whiterabbit"), here), ..WatchContext::default() };
        crate::watch::evaluate(&context)
    };
    let (lander, other) = (seen_by("whiterabbit"), seen_by("gex45"));
    let mut keys: Vec<&str> = lander.iter().map(|alert| alert.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["gh:wisp:api-error", "land:north:runtime-not-live:abc"]);
    assert!(other.is_empty(), "{other:?}");
}
