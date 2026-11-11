use super::{Estimate, Judged, Spending, State, judge};

const ISSUE: &str = "north#385";

fn worker(name: &str, spent: f64, p90: f64) -> Spending {
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
        estimate: Some(Estimate { p50: Some(p90 / 2.0), p90, minutes: 30.0 }),
        landed: 0,
    }
}

fn tick(now_ms: u64, sessions: &[Spending], state: &State) -> Judged {
    judge(now_ms, sessions, state)
}

fn umbrella_box(name: &str, box_name: &str, spent: f64, p90: f64) -> Spending {
    Spending { workstream: "north#112".to_owned(), box_id: format!("north#112 {box_name}"), parent: Some("lead-112".to_owned()), estimate: Some(Estimate { p50: Some(10.83), p90, minutes: 30.0 }), ..worker(name, spent, p90) }
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
    let first = tick(1_000, sessions, &State::default());
    tick(2_000, sessions, &first.state)
}

#[test]
fn one_umbrella_box_past_its_own_p90_alerts_and_an_early_spend_under_p90_is_told_to_its_worker_alone() {
    let warned = twice(&umbrella(40.0));
    assert_eq!(warned.alerts, ["Budget warning: north#112 Proxy role R1: role on the session record has spent $40.00 against E p50 $10.83, p90 $18.29, 2x its p90."], "{warned:?}");
    assert!(warned.events.iter().all(|event| event.scope != "workstream:north#112"), "the issue's $69.65 is within its boxes' summed p90");
    let early = twice(&fast(0.6));
    assert_eq!(told(&early), ["n637"], "{early:?}");
    assert!(early.tells[0].text.contains("past half of p50 in under a quarter of the expected minutes"), "{early:?}");
    let past = twice(&fast(2.5));
    assert_eq!(told(&past), ["n637", "lead-385"], "{past:?}");
    assert!(past.tells[1].text.contains("past its p90"), "{past:?}");
}

fn fast(spent: f64) -> Vec<Spending> {
    vec![Spending { minutes: 2.0, ..worker("n637", spent, 2.0) }]
}

fn told(judged: &Judged) -> Vec<&str> {
    judged.tells.iter().map(|tell| tell.to.as_str()).collect()
}

fn north_706(name: &str, spent: f64, (p50, p90): (f64, f64), landed: u32) -> Spending {
    Spending { workstream: "tompassarelli/north#706".to_owned(), box_id: format!("tompassarelli/north#706@{name}"), parent: Some("proxy".to_owned()), minutes: 80.0, estimate: Some(Estimate { p50: Some(p50), p90, minutes: 72.0 }), landed, ..worker(name, spent, p90) }
}

fn sandbox(wipe: f64, landed: u32) -> Vec<Spending> {
    vec![north_706("n706-wipe", wipe, (3.62, 11.65), landed), north_706("n706-guards2", 19.07, (9.39, 76.74), 0)]
}

#[test]
fn a_session_handed_more_boxes_is_judged_against_each_and_one_box_past_twice_its_p90_still_warns() {
    let before = twice(&sandbox(28.53, 1));
    let after = tick(3_000, &sandbox(29.20, 2), &before.state);
    let after = tick(4_000, &sandbox(29.20, 2), &after.state);
    assert!([&before, &after].iter().all(|judged| judged.alerts.is_empty() && judged.tells.is_empty()), "{before:?} {after:?}");
    let overrun = twice(&sandbox(28.53, 0));
    assert_eq!(overrun.alerts, ["Budget warning: tompassarelli/north#706@n706-wipe has spent $28.53 against E p50 $3.62, p90 $11.65, 2x its p90."], "{overrun:?}");
}

