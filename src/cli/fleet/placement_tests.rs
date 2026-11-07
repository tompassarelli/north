use super::*;
use crate::pacing::placement::gate::{self as placement, Verdict};
use crate::pacing::placement::room::{PlanSize, Rank, Room};

fn verdict(account: &str, headroom: Option<f64>) -> Verdict {
    let line = match headroom {
        Some(room) => format!("{account} available: 5h {:.0}% ({room:.0}% headroom)", 100.0 - room),
        None => format!("{account} unavailable: 5h window at the provider's limit"),
    };
    Verdict { account: account.into(), available: headroom.is_some(), exhausted: headroom.is_none(), headroom: headroom.unwrap_or(0.0), rank: Rank::of(Room { five_hour: headroom.unwrap_or(0.0), week: headroom.unwrap_or(0.0), week_reset_h: 100.0 }, PlanSize { five_hour: 1.0, week: 1.0 }), line, owners: Vec::new() }
}

fn decision(target: Target) -> Decision {
    Decision {
        target,
        code_only: true,
        surface: "code-only box on public north".into(),
        verdicts: vec![verdict("claude:a", Some(60.0)), verdict("claude:c", None)],
        machine: None,
        here: "here".into(),
        here_lacks: None,
        machine_lines: Vec::new(),
        saturated: None,
        here_saturated: None,
        here_cpu_held: None,
        here_on_battery: None,
        crowded: Vec::new(),
        here_closed: None,
        open: Vec::new(),
        moved: None,
    }
}

#[test]
fn an_account_at_its_provider_limit_refuses_even_a_pin_and_names_the_account_placement_picks() {
    let placed = decision(Target::Account("claude:a".into()));
    let launch = Launch { name: "burndown-lead", account: "c", brief: None, cwd: None, machine: None, lead: false };
    let revived = Overrides::default().recorded(Some("pin: north dispatch ranks claude:c first"));
    let refused = admit(&placed, &launch, "", &revived).unwrap_err().to_string();
    assert!(refused.starts_with("provider limit: claude:c unavailable: 5h window at the provider's limit"), "{refused}");
    assert!(refused.contains("placement picks claude:a (claude:a available") && refused.ends_with("launch with --account a"), "{refused}");

    let mut week = placed;
    week.verdicts[1] = Verdict { line: "claude:c unavailable: week window at the provider's limit".into(), ..week.verdicts[1].clone() };
    let local = Overrides { local: Some("x".into()), pin: Some("y".into()), ..Overrides::default() };
    assert!(admit(&week, &launch, "", &local).unwrap_err().to_string().starts_with("provider limit: claude:c unavailable: week window at the provider's limit; placement picks claude:a"));

    let mut all_out = decision(Target::Held("no account".into()));
    let week_out = placement::verdict("claude:c", &serde_json::json!([{"window": "week", "used_pct": 100.0}]));
    assert!(week_out.exhausted && !week_out.available, "{week_out:?}");

    all_out.verdicts.retain(|verdict| verdict.exhausted);
    assert!(admit(&all_out, &launch, "", &revived).unwrap_err().to_string().ends_with("no Claude account is available; wait for a window to reset"));
}


#[test]
fn a_lead_launched_on_a_worker_host_or_onto_a_worker_machine_is_refused_with_the_reason_that_a_lead_runs_where_it_places() {
    let trusted = |host: &str| matches!(host, "whiterabbit" | "nexus");
    let lead = |machine| Launch { name: "n-state-machines", account: "api", brief: None, cwd: Some(Path::new("/home/tom/code/north/main")), machine, lead: true };
    let on_worker_host = lead_refusal("cpx62", &lead(None), trusted).unwrap();
    assert!(on_worker_host.starts_with("cpx62 is a worker host: a lead staffs other hosts") && on_worker_host.ends_with("launch n-state-machines from the laptop or nexus"), "{on_worker_host}");
    assert!(lead_refusal("whiterabbit", &lead(Some("gex45")), trusted).is_some_and(|why| why.starts_with("gex45 is a worker host")));
    assert_eq!(lead_refusal("whiterabbit", &lead(None), trusted), None);
    assert_eq!(lead_refusal("cpx62", &Launch { lead: false, ..lead(None) }, trusted), None, "a worker keeps running on a worker host");
}

#[test]
fn a_pin_keeps_a_saturated_machine_but_queues_on_it_instead_of_starting() {
    let gex45 = placement::Machine { name: "gex45".into(), ssh: "tom@gex45".into(), serves: Vec::new(), pushes: None, reaches: None, launches: true, cap: 8, live: Some(5), accounts: vec!["a".into()], claude_ai: Vec::new(), saturation: None };
    let mut full = decision(Target::Account("claude:a".into()));
    full.machine = Some(gex45);
    full.saturated = Some("gex45 skipped: 4 jobs queued in machine-capacity admission (limit 4)".into());
    let launch = Launch { name: "gex45-n757-leases", account: "a", brief: None, cwd: Some(Path::new("/home/tom/code/north/main")), machine: Some("gex45"), lead: false };
    let pinned = Overrides { pin: Some("continues the lead's wave on gex45".into()), ..Overrides::default() };
    let mut placed = full;
    fall_through(&mut placed, &launch, &pinned);
    assert_eq!(placed.machine.as_ref().map(|machine| machine.name.as_str()), Some("gex45"));
    match admit(&placed, &launch, "", &pinned) {
        Err(Refusal::Saturated(why)) => assert!(why.starts_with("gex45 skipped: 4 jobs queued in machine-capacity admission (limit 4); it starts there once the host has room"), "{why}"),
        other => panic!("a pinned launch onto a saturated machine started: {other:?}"),
    }
}
