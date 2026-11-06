use super::{Answer, Estimate, Judged, Spending, State, judge};

const ISSUE: &str = "north#385";

fn worker(name: &str, spent: f64, created_ms: u64, p90: f64) -> Spending {
    Spending {
        name: name.to_owned(),
        parent: Some("lead-385".to_owned()),
        workstream: ISSUE.to_owned(),
        box_id: format!("{ISSUE}@/lane"),
        lead: false,
        tom: false,
        live: true,
        spent: Some(spent),
        minutes: 30.0,
        created_ms,
        estimate: Some(Estimate { p50: Some(p90 / 2.0), p90, minutes: 30.0 }),
    }
}

fn tick(now_ms: u64, sessions: &[Spending], answers: &[Answer], state: &State) -> Judged {
    judge(now_ms, sessions, answers, state)
}

fn kinds(judged: &Judged) -> Vec<(&'static str, String)> {
    judged.events.iter().map(|event| (event.kind, event.scope.clone())).collect()
}

/// North#385 ran up $1.86 against a p90 of $0.37, 5.0x.
fn replay() -> (Judged, Judged, Judged) {
    let early = tick(1_000, &[worker("n385", 0.20, 0, 0.37)], &[], &State::default());
    let first = tick(2_000, &[worker("n385", 1.86, 0, 0.37)], &[], &early.state);
    let second = tick(3_000, &[worker("n385", 1.90, 0, 0.37)], &[], &first.state);
    (early, first, second)
}

// KEEP(bug): a run past four times its p90 trips on the second tick and not the first; replays the north#385 overrun
#[test]
fn a_run_past_four_times_p90_trips_on_the_second_tick_and_not_the_first() {
    let (early, first, second) = replay();
    assert!(early.events.is_empty() && early.tells.is_empty());
    assert!(first.events.iter().all(|event| event.kind != "trip") && first.tells.is_empty() && first.alerts.is_empty(), "{first:?}");
    assert!(kinds(&second).contains(&("trip", format!("workstream:{ISSUE}"))), "{second:?}");
    assert_eq!(second.alerts.len(), 1, "Tom sees the trip once");
}

fn umbrella_box(name: &str, box_name: &str, spent: f64, p90: f64) -> Spending {
    Spending { workstream: "north#112".to_owned(), box_id: format!("north#112 {box_name}"), parent: Some("lead-112".to_owned()), estimate: Some(Estimate { p50: Some(10.83), p90, minutes: 30.0 }), ..worker(name, spent, 0, p90) }
}

/// North#112's boxes on 2026-11-05 evening: $36.65 together against one box's p90 of $18.29, each under its own.
fn umbrella(r1: f64) -> Vec<Spending> {
    vec![
        umbrella_box("r1", "Proxy role R1: role on the session record", r1, 18.29),
        umbrella_box("r2", "Proxy role R2: role prompt and grounding", 8.00, 18.29),
        umbrella_box("r3", "Proxy role R3", 6.50, 18.29),
        umbrella_box("f1", "Proxy role F1", 7.15, 18.29),
        umbrella_box("incidents", "incidents by construction", 8.00, 18.29),
    ]
}

fn twice(sessions: &[Spending]) -> Judged {
    let first = tick(1_000, sessions, &[], &State::default());
    tick(2_000, sessions, &[], &first.state)
}

// KEEP(bug): one umbrella box past its own p90 alerts though the issue is within its sum, regression of 823d9a434 (north#592); an early spend under p90 reached the proxy seven times between 4:00 and 4:50 PM on 2026-11-06 (north#637)
#[test]
fn one_umbrella_box_past_its_own_p90_alerts_and_an_early_spend_under_p90_is_told_to_its_worker_alone() {
    let warned = twice(&umbrella(40.0));
    assert_eq!(warned.alerts, ["Budget warning: north#112 Proxy role R1: role on the session record has spent $40.00 against E p50 $10.83, p90 $18.29, 2x its p90."], "{warned:?}");
    assert!(warned.events.iter().all(|event| event.scope != "workstream:north#112"), "the issue's $69.65 is within its boxes' summed p90");
    let tripped = twice(&umbrella(80.0));
    assert_eq!(tripped.alerts.len(), 1, "{tripped:?}");
    assert!(tripped.alerts[0].starts_with("Budget tripped: north#112 Proxy role R1"));
    assert_eq!(tripped.tells.iter().map(|tell| tell.to.as_str()).collect::<Vec<_>>(), ["r1"], "only the box past its p90 is told to stop");
    let early = twice(&fast(0.6));
    assert_eq!(told(&early), ["n637"], "{early:?}");
    assert!(early.tells[0].text.contains("past half of p50 in under a quarter of the expected minutes"), "{early:?}");
    let past = twice(&fast(2.5));
    assert_eq!(told(&past), ["n637", "lead-385"], "{past:?}");
    assert!(past.tells[1].text.contains("past its p90"), "{past:?}");
}

fn fast(spent: f64) -> Vec<Spending> {
    vec![Spending { minutes: 2.0, ..worker("n637", spent, 0, 2.0) }]
}

fn told(judged: &Judged) -> Vec<&str> {
    judged.tells.iter().map(|tell| tell.to.as_str()).collect()
}

