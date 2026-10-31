use super::*;
use crate::ops::pacing::placement::gate::{self as placement, Verdict};
use crate::ops::pacing::placement::room::{PlanSize, Rank, Room};

fn verdict(account: &str, headroom: Option<f64>) -> Verdict {
    let line = match headroom {
        Some(room) => format!("{account} available: 5h {:.0}% ({room:.0}% headroom)", 95.0 - room),
        None => format!("{account} unavailable: burning: 5h 86.0%/h > the 50%/h cap that keeps the window two hours"),
    };
    Verdict { account: account.into(), available: headroom.is_some(), exhausted: headroom.is_none(), headroom: headroom.unwrap_or(0.0), rank: Rank::of(Room { five_hour: headroom.unwrap_or(0.0), week: headroom.unwrap_or(0.0), week_reset_h: 100.0 }, PlanSize { five_hour: 1.0, week: 1.0 }, false), line, owners: Vec::new() }
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
    }
}

fn noted(note: Option<&str>) -> Result<Placed, Refusal> {
    Ok(Placed { note: note.map(Into::into), machine: None, need: None })
}

#[test]
fn an_override_reason_admits_an_off_placement_launch_and_survives_revive() {
    let mut cloud = decision(Target::Cloud("cloud credit: unread".into()));
    let mut local = decision(Target::Account("claude:a".into()));
    for decision in [&mut cloud, &mut local] {
        decision.verdicts.push(verdict("claude:b", Some(30.0)));
    }
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: None };
    let given = |local: Option<&str>, pin: Option<&str>| Overrides { local: local.map(Into::into), pin: pin.map(Into::into), ..Overrides::default() };

    assert_eq!(admit(&local, &launch("a"), "", &Overrides::default()), noted(None));
    let refused = admit(&local, &launch("c"), "", &Overrides::default()).unwrap_err().to_string();
    assert!(refused.starts_with("claude:c unavailable: burning") && refused.contains("--account a"), "{refused}");
    assert_eq!(admit(&local, &launch("b"), "", &given(None, Some("drain b"))), noted(Some("pin: drain b")));
    assert!(admit(&cloud, &launch("a"), "", &given(None, Some("drain c"))).unwrap_err().to_string().contains("north cloud launch"));
    assert!(admit(&cloud, &launch("b"), "", &given(Some("x"), None)).unwrap_err().to_string().contains("--pin"));
    let both = admit(&cloud, &launch("b"), "", &given(Some("x"), Some("y"))).unwrap().note;
    assert_eq!(both.as_deref(), Some("local: x; pin: y"));

    assert_eq!(Overrides::default().recorded(both.as_deref()), given(Some("x"), Some("y")));
    assert_eq!(given(Some("new"), None).recorded(both.as_deref()), given(Some("new"), Some("y")));
    assert_eq!(Overrides::default().recorded(None), Overrides::default());
    let tiered = Overrides { tier: Some("the cause spans two engines".into()), ..given(None, Some("y")) };
    let note = admit(&local, &launch("b"), "", &tiered).unwrap().note;
    assert_eq!(note.as_deref(), Some("pin: y; tier: the cause spans two engines"));
    assert_eq!(Overrides::default().recorded(note.as_deref()), tiered.clone(), "revive keeps the tier reason");

    let mut remote = decision(Target::Account("claude:a".into()));
    remote.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, cap: 4, live: Some(1), accounts: vec!["a".into(), "c".into()], saturation: None });
    let launch = Launch { name: "box", account: "a", brief: None, cwd: None, machine: None };
    let placed = admit(&remote, &launch, "", &Overrides::default()).unwrap();
    assert_eq!(placed, Placed { note: None, machine: Some(Machine { name: "far".into(), ssh: "tom@far".into() }), need: None });
    let kept = admit(&remote, &launch, "", &Overrides { local: Some("needs my desktop".into()), pin: None, ..Overrides::default() }).unwrap();
    assert_eq!(kept, Placed { note: Some("local: needs my desktop".into()), machine: None, need: None });
    let here = admit(&decision(Target::Account("claude:a".into())), &launch, "", &Overrides { local: Some("x".into()), pin: None, ..Overrides::default() }).unwrap();
    assert_eq!(here, noted(None).unwrap(), "--local on a box placed here records nothing");

    let unsigned = admit(&remote, &Launch { name: "box", account: "b", brief: None, cwd: None, machine: None }, "", &Overrides::default()).unwrap_err().to_string();
    assert!(unsigned.starts_with("far is not signed in to claude:b (signed in there: a, c); launch with --account a, or "), "{unsigned}");
    let pinned = admit(&remote, &Launch { name: "box", account: "b", brief: None, cwd: None, machine: None }, "", &Overrides { local: Some("x".into()), pin: Some("y".into()), ..Overrides::default() });
    assert_eq!(pinned.unwrap().machine, None);
}

#[test]
fn the_account_placement_resolved_for_the_machine_needs_no_pin_beside_a_roomier_one_elsewhere() {
    let mut remote = decision(Target::Account("claude:a".into()));
    remote.verdicts.push(verdict("claude:b", Some(90.0)));
    remote.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, cap: 4, live: Some(0), accounts: vec!["a".into()], saturation: None });
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: Some("far") };
    assert_eq!(admit(&remote, &launch("a"), "", &Overrides::default()).unwrap().note, None);
    let here = decision(Target::Account("claude:a".into()));
    let mut roomier = here.clone();
    roomier.verdicts.push(verdict("claude:b", Some(90.0)));
    assert!(admit(&roomier, &launch("c"), "", &Overrides::default()).unwrap_err().to_string().contains("placement picks claude:a"));
    let mut cloud = decision(Target::Cloud("cloud credit: unread".into()));
    cloud.verdicts = vec![Verdict { owners: vec!["nexus".into()], ..verdict("claude:a", Some(90.0)) }, verdict("claude:b", Some(40.0))];
    let kept = admit(&cloud, &launch("b"), "", &Overrides { local: Some("reads a local lane".into()), pin: None, ..Overrides::default() }).unwrap();
    assert_eq!(kept.note.as_deref(), Some("local: reads a local lane"), "a box kept here needs no pin beside an account that runs only elsewhere");
}

#[test]
fn an_available_account_with_less_headroom_than_placements_pick_is_admitted_only_with_a_pin() {
    let mut roomier = decision(Target::Account("claude:a".into()));
    roomier.verdicts.push(verdict("claude:b", Some(30.0)));
    let launch = Launch { name: "box", account: "b", brief: None, cwd: None, machine: None };
    assert!(admit(&roomier, &launch, "", &Overrides::default()).unwrap_err().to_string().contains("placement picks claude:a"));
    let pin = Overrides { pin: Some("north dispatch ranks claude:b first by spend target and week room expiring soonest".into()), ..Overrides::default() };
    assert!(admit(&roomier, &launch, "", &pin).unwrap().note.unwrap().starts_with("pin: north dispatch ranks claude:b first"));
}

#[test]
fn the_account_the_shared_rank_puts_first_is_admitted_without_a_pin_though_another_has_more_headroom() {
    let mut ranked = decision(Target::Account("claude:b".into()));
    ranked.verdicts = vec![verdict("claude:a", Some(80.0)), verdict("claude:b", Some(30.0))];
    ranked.verdicts[1].rank = Rank::of(Room { five_hour: 30.0, week: 30.0, week_reset_h: 5.0 }, PlanSize { five_hour: 1.0, week: 1.0 }, false);
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: None };
    assert_eq!(admit(&ranked, &launch("b"), "", &Overrides::default()), noted(None), "b's week expires sooner, so placement ranks it first");
    assert!(admit(&ranked, &launch("a"), "", &Overrides::default()).unwrap_err().to_string().contains("placement picks claude:b"));
}

#[test]
fn a_launch_on_a_saturated_named_machine_needs_a_pin_and_a_local_keep_does_not() {
    let mut saturated = decision(Target::Account("claude:a".into()));
    saturated.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, cap: 4, live: Some(1), accounts: vec!["a".into()], saturation: None });
    saturated.saturated = Some("far skipped: 9 queued for admission".into());
    let launch = Launch { name: "worker", account: "a", brief: None, cwd: None, machine: Some("far") };
    let refused = admit(&saturated, &launch, "", &Overrides::default()).unwrap_err();
    assert!(matches!(refused, Refusal::Saturated(_)), "a saturated machine is a refusal a waiting launch outlasts: {refused:?}");
    let refused = refused.to_string();
    assert!(refused.starts_with("far skipped: 9 queued for admission"), "{refused}");
    let pinned = Overrides { pin: Some("Tom named it".into()), ..Overrides::default() };
    let placed = admit(&saturated, &launch, "", &pinned).unwrap();
    assert_eq!(placed.note.as_deref(), Some("pin: Tom named it"));
    assert_eq!(placed.machine.map(|machine| machine.name), Some("far".to_owned()));
    let local = Overrides { local: Some("laptop work".into()), ..Overrides::default() };
    let kept = admit(&saturated, &launch, "", &local).unwrap();
    assert_eq!((kept.note.as_deref(), kept.machine), (Some("local: laptop work"), None));
}

#[test]
fn a_box_kept_on_a_saturated_launching_host_needs_local_or_a_pin_and_a_move_elsewhere_does_not() {
    let why = "here skipped: 14.0 GiB free on the root disk, under 15";
    let mut held = decision(Target::Account("claude:a".into()));
    held.here_saturated = Some(why.into());
    held.hold_here();
    assert_eq!(held.target, Target::Held(why.into()));
    assert_eq!(held.machine_lines, [why]);
    let launch = Launch { name: "worker", account: "a", brief: None, cwd: None, machine: None };
    let refused = admit(&held, &launch, "", &Overrides::default()).unwrap_err();
    assert!(matches!(refused, Refusal::Saturated(_)), "{refused:?}");
    let refused = refused.to_string();
    assert!(refused.starts_with(why), "{refused}");
    let local = Overrides { local: Some("this host is the one that serves it".into()), ..Overrides::default() };
    assert_eq!(admit(&held, &launch, "", &local).unwrap().note.as_deref(), Some("local: this host is the one that serves it"));
    let pinned = Overrides { pin: Some("urgent".into()), ..Overrides::default() };
    assert_eq!(admit(&held, &launch, "", &pinned).unwrap().note.as_deref(), Some("pin: urgent"));

    let mut elsewhere = decision(Target::Account("claude:a".into()));
    elsewhere.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, cap: 4, live: Some(1), accounts: vec!["a".into()], saturation: None });
    elsewhere.here_saturated = Some(why.into());
    elsewhere.hold_here();
    assert_eq!(elsewhere.target, Target::Account("claude:a".into()));
    assert_eq!(admit(&elsewhere, &launch, "", &Overrides::default()).unwrap().machine.map(|machine| machine.name), Some("far".to_owned()));
}

#[test]
fn an_account_that_is_out_refuses_even_a_pin_and_names_the_account_placement_picks() {
    let placed = decision(Target::Account("claude:a".into()));
    let launch = Launch { name: "burndown-lead", account: "c", brief: None, cwd: None, machine: None };
    let revived = Overrides::default().recorded(Some("pin: north dispatch ranks claude:c first"));
    let refused = admit(&placed, &launch, "", &revived).unwrap_err().to_string();
    assert!(refused.starts_with("claude:c unavailable: burning: 5h 86.0%/h"), "{refused}");
    assert!(refused.contains("placement picks claude:a (claude:a available") && refused.ends_with("launch with --account a"), "{refused}");

    let mut week = placed.clone();
    week.verdicts[1] = Verdict { line: "claude:c unavailable: exhausted: provider week 100%".into(), ..week.verdicts[1].clone() };
    let local = Overrides { local: Some("x".into()), pin: Some("y".into()), ..Overrides::default() };
    assert!(admit(&week, &launch, "", &local).unwrap_err().to_string().starts_with("claude:c unavailable: exhausted: provider week 100%; placement picks claude:a"));

    let mut all_out = decision(Target::Held("no account".into()));
    let week_out = placement::verdict("claude:c", &serde_json::json!({"windows": [{"window": "week", "used_pct": 100.0}]}));
    assert!(week_out.exhausted && !week_out.available, "{week_out:?}");

    all_out.verdicts.retain(|verdict| verdict.exhausted);
    assert!(admit(&all_out, &launch, "", &revived).unwrap_err().to_string().ends_with("no Claude account is available; wait for a window to reset"));
}

#[test]
fn a_named_account_within_the_tie_band_of_the_best_is_admitted_without_a_pin_and_one_ten_points_behind_is_not() {
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: None };
    let tied = |best: &str| Decision { verdicts: vec![verdict("claude:a", Some(28.0)), verdict("claude:c", Some(30.0)), verdict("claude:b", Some(20.0))], ..decision(Target::Account(best.into())) };
    for best in ["claude:a", "claude:c"] {
        for account in ["a", "c"] {
            assert_eq!(admit(&tied(best), &launch(account), "", &Overrides::default()), noted(None), "--account {account} when placement picks {best}");
        }
    }
    let refused = admit(&tied("claude:c"), &launch("b"), "", &Overrides::default()).unwrap_err().to_string();
    assert!(refused.contains("placement picks claude:c") && refused.contains("--pin"), "{refused}");
}
