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
fn a_land_check_with_no_lane_raises_no_alarm_and_a_real_lane_past_its_p90_still_does() {
    let mut laneless = context(3.0, 221_105.0);
    laneless.events[0].lane = String::new();
    assert!(stalls(&laneless).is_empty(), "no lane is being checked, so nothing is running past its p90");
    let mut quiet = context(3.0, 221_105.0);
    quiet.events[0].lane = String::new();
    quiet.events[0].status = "start".into();
    assert!(stalls(&quiet).is_empty(), "a laneless event is not a stalled safe-push either");
    let mut mixed = context(3.0, 700.0);
    mixed.events.push(Event { time: NOW as f64 - 221_105.0, repo: "north".into(), stage: "land checks".into(), status: "wait".into(), pid: 7, ..Event::default() });
    mixed.events.reverse();
    let alerts = stalls(&mixed);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert!(alerts[0].summary.contains("lane lane-a,lane-b:"), "{}", alerts[0].summary);
}

#[test]
fn two_hosts_seeing_one_github_event_alarm_only_from_the_lander_host() {
    use crate::watch::facts::RepoFacts;
    use north_base::platform::harness::defers_to;
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

#[test]
fn a_landing_behind_a_follower_still_building_is_not_paged() {
    let settle = serde_json::json!({"landing": "abc", "checkedAt": NOW, "hosts": [{"host": "gex45", "problems": ["not live"]}]});
    let watch = |at: u64, contains_landing: bool| WatchContext {
        now: NOW,
        settle: Some(settle.clone()),
        follow: Some(crate::watch::Follow { sha: "def".into(), contains_landing, at }),
        ..WatchContext::default()
    };
    let building = runtime(&watch(NOW - 60, true));
    assert_eq!(building.len(), 1);
    assert_eq!(building[0].severity, Severity::Info, "the follower is building a main containing the landing: {}", building[0].summary);
    let stalled = runtime(&watch(NOW - FOLLOW_PROGRESS_SECONDS, true));
    assert_eq!(stalled[0].severity, Severity::Warn);
    assert!(stalled[0].summary.contains("no progress for 15 min"), "{}", stalled[0].summary);
    let elsewhere = runtime(&watch(NOW - 60, false));
    assert_eq!(elsewhere[0].severity, Severity::Warn, "a follower building a main without the landing does not hold it");
}

fn iso(unix: u64) -> String {
    let (days, rest) = (unix / 86_400, unix % 86_400);
    let date = time::Date::from_julian_day(i32::try_from(days).unwrap() + 2_440_588).unwrap();
    format!("{date}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[test]
fn a_possible_red_main_alert_clears_once_a_later_main_commit_passes_the_cargo_run_and_not_before() {
    use crate::watch::facts::{RepoFacts, Run};
    let failed = |lane: &str, ago: u64| Event { time: (NOW - ago) as f64, repo: "north".into(), lane: lane.into(), stage: "land check".into(), status: "failed".into(), tests: vec!["agent_text_reads".into()], ..Event::default() };
    let cargo = |conclusion: &str, ago: u64| Run { name: "Cargo".into(), head_branch: "main".into(), event: "push".into(), status: "completed".into(), conclusion: Some(conclusion.into()), created_at: iso(NOW - ago), ..Run::default() };
    let watch = |runs: Vec<Run>| WatchContext { now: NOW, events: vec![failed("lane-a", 1500), failed("lane-b", 1200)], repos: vec![RepoFacts { name: "north".into(), runs, ..RepoFacts::default() }], ..WatchContext::default() };
    let keys = |context: &WatchContext| shared_failures(context).into_iter().map(|alert| alert.key).collect::<Vec<_>>();
    assert_eq!(keys(&watch(vec![])), ["land:north:possible-red-main:agent_text_reads"]);
    assert_eq!(keys(&watch(vec![cargo("success", 1800), cargo("failure", 600)])), ["land:north:possible-red-main:agent_text_reads"], "a pass before the failures or a red run after them does not clear it");
    assert!(keys(&watch(vec![cargo("failure", 900), cargo("success", 300)])).is_empty(), "a later main passing Cargo clears it on the next tick");
}
