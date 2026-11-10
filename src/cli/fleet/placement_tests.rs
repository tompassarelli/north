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
        family: None,
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
fn a_coordinator_launch_with_no_fable_headroom_takes_opus_high_on_the_account_placement_chose() {
    let mut fallback = decision(Target::Account("claude:b".into()));
    fallback.family = Some(ModelFamily::Opus);
    fallback.verdicts.push(verdict("claude:b", Some(40.0)));
    let launch = Launch { name: "proxy", account: "b", brief: None, cwd: None, machine: None, lead: false, role: Role::parse("proxy").unwrap(), model: Some("fable"), family: ModelFamily::Fable };
    let placed = settle(&fallback, &launch, "", &Overrides::default()).unwrap();
    assert_eq!((placed.model.as_deref(), placed.effort), (Some("opus"), Some(Effort::High)), "{placed:?}");
    assert!(placed.note.as_deref().is_some_and(|note| note.contains("Opus high, no account has Fable headroom")), "{placed:?}");
}

#[test]
fn an_account_at_its_provider_limit_refuses_even_a_pin_and_names_the_account_placement_picks() {
    let placed = decision(Target::Account("claude:a".into()));
    let launch = Launch { name: "burndown-lead", account: "c", brief: None, cwd: None, machine: None, lead: false, role: Role::Worker, model: None, family: ModelFamily::Opus };
    let revived = Overrides::default().recorded(Some("pin: north fleet dispatch ranks claude:c first"));
    let refused = admit(&placed, &launch, "", &revived).unwrap_err().to_string();
    assert!(refused.starts_with("provider limit: claude:c unavailable: 5h window at the provider's limit"), "{refused}");
    assert!(refused.contains("placement picks claude:a (claude:a available") && refused.ends_with("launch with --account a"), "{refused}");

    let mut week = placed;
    week.verdicts[1] = Verdict { line: "claude:c unavailable: week window at the provider's limit".into(), ..week.verdicts[1].clone() };
    let local = Overrides { local: Some("x".into()), pin: Some("y".into()), ..Overrides::default() };
    assert!(admit(&week, &launch, "", &local).unwrap_err().to_string().starts_with("provider limit: claude:c unavailable: week window at the provider's limit; placement picks claude:a"));

    let mut all_out = decision(Target::Held("no account".into()));
    let week_out = placement::verdict("claude:c", &serde_json::json!([{"window": "week", "used_pct": 100.0}]), None);
    assert!(week_out.exhausted && !week_out.available, "{week_out:?}");

    all_out.verdicts.retain(|verdict| verdict.exhausted);
    assert!(admit(&all_out, &launch, "", &revived).unwrap_err().to_string().ends_with("no Claude account is available; wait for a window to reset"));
}


#[test]
fn a_lead_launched_on_a_worker_host_or_onto_a_worker_machine_is_refused_with_the_reason_that_a_lead_runs_where_it_places() {
    let trusted = |host: &str| matches!(host, "whiterabbit" | "nexus");
    let lead = |machine| Launch { name: "n-state-machines", account: "api", brief: None, cwd: Some(Path::new("/home/tom/code/north/main")), machine, lead: true, role: Role::Lead, model: None, family: ModelFamily::Opus };
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
    let launch = Launch { name: "gex45-n757-leases", account: "a", brief: None, cwd: Some(Path::new("/home/tom/code/north/main")), machine: Some("gex45"), lead: false, role: Role::Worker, model: None, family: ModelFamily::Opus };
    let pinned = Overrides { pin: Some("continues the lead's wave on gex45".into()), ..Overrides::default() };
    let mut placed = full;
    fall_through(&mut placed, &launch, &pinned);
    assert_eq!(placed.machine.as_ref().map(|machine| machine.name.as_str()), Some("gex45"));
    match admit(&placed, &launch, "", &pinned) {
        Err(Refusal::Saturated(why)) => assert!(why.starts_with("gex45 skipped: 4 jobs queued in machine-capacity admission (limit 4); it starts there once the host has room"), "{why}"),
        other => panic!("a pinned launch onto a saturated machine started: {other:?}"),
    }
}

#[test]
fn a_launch_held_because_this_host_is_loaded_is_refused_as_saturated_so_its_caller_retries() {
    let mut loaded = decision(Target::Account("claude:a".into()));
    loaded.here_saturated = Some("whiterabbit skipped: load 41 on 24 threads".into());
    loaded.hold_here();
    match pick(&loaded, &Overrides::default()) {
        Err(Refusal::Saturated(why)) => assert_eq!(why, "held: whiterabbit skipped: load 41 on 24 threads"),
        other => panic!("a launch held for room was not refused as saturated: {other:?}"),
    }
    assert!(matches!(pick(&decision(Target::Held("every Claude account is at its provider's limit".into())), &Overrides::default()), Err(Refusal::Final(_))));
}

fn placed_on(account: &str) -> Placed {
    Placed { account: Some(account.to_owned()), ..Placed::default() }
}

#[test]
fn a_launch_naming_no_account_moves_to_one_the_spend_rule_admits_and_defers_with_exit_75_when_none_is() {
    let admits = |account: &str| match account {
        "b" => Ok(()),
        other => Err(format!("claude:{other} is at its provider's week limit")),
    };
    let moved = with_room("c", &["a", "b", "c"], admits, |account| Ok(Ok(placed_on(account))), Ok(placed_on("c"))).unwrap();
    assert_eq!(moved.unwrap().account.as_deref(), Some("b"));

    let refusal = with_room("c", &["a", "c"], admits, |account| Ok(Ok(placed_on(account))), Ok(placed_on("c"))).unwrap().unwrap_err();
    assert!(matches!(&refusal, Refusal::Spend(why) if why.starts_with("claude:c is at its provider's week limit; claude:a")), "{refusal:?}");
    assert_eq!(NorthError::from(refusal).exit_code(), 75);
}

#[test]
fn a_deepseek_launch_on_a_saturated_host_is_refused_as_saturated_and_queued_instead_of_starting() {
    use crate::capacity::saturation::{Backlog, Saturation};
    let reading = |queued, load| Saturation { memory_total_mib: 98_304.0, memory_available_mib: 61_000.0, disk_free_gib: 400.0, disk_total_gib: 936.0, backlog: Some(Backlog { queued, load, threads: 24, stall: None }), token_age_s: None };
    let refused = harness_host("whiterabbit", Some(&reading(4, 8.0))).unwrap_err();
    assert!(matches!(&refused, Refusal::Saturated(why) if why.starts_with("whiterabbit skipped: 4 jobs queued in machine-capacity admission")), "{refused:?}");
    assert!(refused.queues("opencode") && !refused.queues("claude"));
    assert!(matches!(harness_host("whiterabbit", Some(&reading(0, 31.0))), Err(Refusal::Saturated(why)) if why.starts_with("whiterabbit skipped: load 31 on 24 threads")));
    assert_eq!(harness_host("whiterabbit", Some(&reading(0, 8.0))), Ok(Placed::default()));
    assert_eq!(harness_host("whiterabbit", None), Ok(Placed::default()), "an unreadable host refuses nothing");
}
