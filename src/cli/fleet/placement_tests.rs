use super::*;
use crate::pacing::placement::gate::{self as placement, CloudLane, Verdict};
use crate::pacing::placement::room::{PlanSize, Rank, Room};

fn verdict(account: &str, headroom: Option<f64>) -> Verdict {
    let line = match headroom {
        Some(room) => format!("{account} available: 5h {:.0}% ({room:.0}% headroom)", 95.0 - room),
        None => format!("{account} unavailable: burning: 5h 86.0%/h > the 50%/h cap that keeps the window two hours"),
    };
    Verdict { account: account.into(), available: headroom.is_some(), exhausted: headroom.is_none(), headroom: headroom.unwrap_or(0.0), rank: Rank::of(Room { five_hour: headroom.unwrap_or(0.0), week: headroom.unwrap_or(0.0), week_reset_h: 100.0 }, PlanSize { five_hour: 1.0, week: 1.0 }, false), line, owners: Vec::new() }
}

fn cloud(account: &str, host: &str) -> Target {
    Target::Cloud(CloudLane { account: format!("claude:{account}"), host: host.into(), credit: "cloud credit: spent (read 2026-10-26), expires in 1 days".into() })
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

fn noted(note: Option<&str>) -> Result<Placed, Refusal> {
    Ok(Placed { account: None, note: note.map(Into::into), machine: None, need: None })
}

#[test]
fn an_override_reason_admits_an_off_placement_launch_and_survives_revive() {
    let mut cloud = decision(cloud("a", "here"));
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
    assert_eq!(Overrides::default().recorded(note.as_deref()), tiered, "revive keeps the tier reason");

    let mut remote = decision(Target::Account("claude:a".into()));
    remote.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: Some(1), accounts: vec!["a".into(), "c".into()], saturation: None });
    let launch = Launch { name: "box", account: "a", brief: None, cwd: None, machine: None };
    let placed = admit(&remote, &launch, "", &Overrides::default()).unwrap();
    assert_eq!(placed, Placed { account: None, note: None, machine: Some(Machine { name: "far".into(), ssh: "tom@far".into() }), need: None });
    let kept = admit(&remote, &launch, "", &Overrides { local: Some("needs my desktop".into()), pin: None, ..Overrides::default() }).unwrap();
    assert_eq!(kept, Placed { account: None, note: Some("local: needs my desktop".into()), machine: None, need: None });
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
    remote.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: Some(0), accounts: vec!["a".into()], saturation: None });
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: Some("far") };
    assert_eq!(admit(&remote, &launch("a"), "", &Overrides::default()).unwrap().note, None);
    let here = decision(Target::Account("claude:a".into()));
    let mut roomier = here;
    roomier.verdicts.push(verdict("claude:b", Some(90.0)));
    assert!(admit(&roomier, &launch("c"), "", &Overrides::default()).unwrap_err().to_string().contains("placement picks claude:a"));
    let mut cloud = decision(cloud("a", "here"));
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
    saturated.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: Some(1), accounts: vec!["a".into()], saturation: None });
    saturated.saturated = Some("far skipped: 14.0 GiB free on the root disk, under 15".into());
    let launch = Launch { name: "worker", account: "a", brief: None, cwd: None, machine: Some("far") };
    let refused = admit(&saturated, &launch, "", &Overrides::default()).unwrap_err();
    assert!(matches!(refused, Refusal::Saturated(_)), "a saturated machine is a refusal a waiting launch outlasts: {refused:?}");
    let refused = refused.to_string();
    assert!(refused.starts_with("far skipped: 14.0 GiB free on the root disk, under 15"), "{refused}");
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
    elsewhere.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: Some(1), accounts: vec!["a".into()], saturation: None });
    elsewhere.here_saturated = Some(why.into());
    elsewhere.hold_here();
    assert_eq!(elsewhere.target, Target::Account("claude:a".into()));
    assert_eq!(admit(&elsewhere, &launch, "", &Overrides::default()).unwrap().machine.map(|machine| machine.name), Some("far".to_owned()));
}

#[test]
fn a_laptop_on_battery_refuses_local_and_a_pin_keeps_it_with_the_reason_recorded() {
    let why = "laptop is a laptop on battery (discharging at 80%), so --local would drain it; launch the box on gex45 or in anthropic-cloud without --local, or keep it here with --pin \"<reason>\"";
    let mut cloud = decision(cloud("a", "laptop"));
    cloud.here_on_battery = Some(why.into());
    let launch = Launch { name: "worker", account: "a", brief: None, cwd: None, machine: None };
    let local = Overrides { local: Some("needs this checkout".into()), ..Overrides::default() };
    let refused = admit(&cloud, &launch, "", &local).unwrap_err();
    assert_eq!(refused, Refusal::Final(why.into()));
    let pinned = Overrides { pin: Some("Tom asked for it here".into()), ..local };
    assert_eq!(admit(&cloud, &launch, "", &pinned), noted(Some("local: needs this checkout; pin: Tom asked for it here")));
    assert!(admit(&cloud, &launch, "", &Overrides::default()).unwrap_err().to_string().contains("north cloud launch"));
}

#[test]
fn an_account_that_is_out_refuses_even_a_pin_and_names_the_account_placement_picks() {
    let placed = decision(Target::Account("claude:a".into()));
    let launch = Launch { name: "burndown-lead", account: "c", brief: None, cwd: None, machine: None };
    let revived = Overrides::default().recorded(Some("pin: north dispatch ranks claude:c first"));
    let refused = admit(&placed, &launch, "", &revived).unwrap_err().to_string();
    assert!(refused.starts_with("claude:c unavailable: burning: 5h 86.0%/h"), "{refused}");
    assert!(refused.contains("placement picks claude:a (claude:a available") && refused.ends_with("launch with --account a"), "{refused}");

    let mut week = placed;
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

#[test]
fn a_cloud_target_prints_the_launch_command_for_its_account_on_the_host_that_holds_the_login() {
    let launch = Launch { name: "box", account: "d", brief: Some(Path::new("/tmp/box.md")), cwd: None, machine: None };
    let text = "Item: north#12 tidy\nSurface: rust\n";
    let local = admit(&decision(cloud("d", "here")), &launch, text, &Overrides::default()).unwrap_err().to_string();
    assert!(local.contains("anthropic-cloud under claude:d") && local.contains("cloud credit: spent") && local.contains("no separate charge"), "{local}");
    assert!(local.contains("cp /tmp/box.md /tmp/cloud/box.md") && !local.contains("ssh"), "{local}");
    assert!(local.contains("CLAUDE_CONFIG_DIR=$HOME/.local/state/north/profiles/claude-d north cloud launch north box /tmp/cloud/box.md"), "{local}");

    let far = admit(&decision(cloud("d", "laptop")), &launch, text, &Overrides::default()).unwrap_err().to_string();
    assert!(far.contains("Run it on laptop, where claude:d is signed in"), "{far}");
    assert!(far.contains("ssh laptop 'mkdir -p /tmp/cloud && cat > /tmp/cloud/box.md' < /tmp/box.md"), "{far}");
    assert!(far.contains("ssh laptop 'CLAUDE_CONFIG_DIR=$HOME/.local/state/north/profiles/claude-d north cloud launch north box /tmp/cloud/box.md'"), "{far}");
    assert!(!far.contains("Nexus instead"), "{far}");
}

#[test]
fn a_launch_naming_no_account_runs_on_the_account_placement_prints_and_a_held_placement_refuses_with_its_reason() {
    let unnamed = Launch { name: "box", account: "", brief: None, cwd: None, machine: None };
    let mut elsewhere = Decision { verdicts: vec![verdict("claude:c", None), verdict("claude:b", Some(40.0)), verdict("claude:a", Some(10.0))], ..decision(Target::Account("claude:b".into())) };
    assert!(placement::summary(&elsewhere).contains("claude:b"));
    let placed = settle(&elsewhere, &unnamed, "", &Overrides::default()).unwrap();
    assert_eq!(placed.account.as_deref(), Some("b"));
    assert_eq!(placed.note, None);

    let pinned = Overrides { pin: Some("keep a".into()), ..Overrides::default() };
    assert_eq!(settle(&elsewhere, &unnamed, "", &pinned).unwrap().account.as_deref(), Some("b"), "a pin never changes the account placement picks");

    let named = Launch { account: "c", ..unnamed };
    assert!(settle(&elsewhere, &named, "", &Overrides::default()).unwrap_err().to_string().contains("placement picks claude:b"), "an explicit exhausted account keeps its refusal");

    elsewhere.target = Target::Held("every Claude account exhausted or over its burn cap".into());
    let held = settle(&elsewhere, &unnamed, "", &Overrides::default()).unwrap_err();
    assert_eq!(held.to_string(), "held: every Claude account exhausted or over its burn cap");
    assert_eq!(NorthError::from(held).user_message(), "held: every Claude account exhausted or over its burn cap");

    let cloud = decision(cloud("a", "nexus"));
    assert!(settle(&cloud, &unnamed, "", &Overrides::default()).unwrap_err().to_string().contains("north cloud launch"));
    let local = Overrides { local: Some("x".into()), ..Overrides::default() };
    assert_eq!(settle(&cloud, &unnamed, "", &local).unwrap().account.as_deref(), Some("a"));
}

#[test]
fn a_local_launch_on_a_host_past_the_spawn_gate_cpu_line_needs_a_pin_and_local_does_not_pass_it() {
    let why = "Capacity leases and unleased heavy load commit 20 of the 16 CPUs the machine can hand out, protected desktop pressure is 5% (limit 20%), and 30 local workers are running.";
    let launch = Launch { name: "worker", account: "a", brief: None, cwd: None, machine: None };
    let mut here = decision(Target::Account("claude:a".into()));
    here.here_cpu_held = Some(why.into());
    let refused = admit(&here, &launch, "", &Overrides::default()).unwrap_err();
    assert!(matches!(&refused, Refusal::Saturated(text) if text.starts_with(why) && text.contains("--pin")), "{refused:?}");
    let local = Overrides { local: Some("needs this checkout".into()), ..Overrides::default() };
    let refused = admit(&here, &launch, "", &local).unwrap_err().to_string();
    assert!(refused.starts_with(why), "--local keeps the box here but never past the CPU line: {refused}");
    let pinned = Overrides { pin: Some("Tom asked for it here".into()), ..local.clone() };
    assert_eq!(admit(&here, &launch, "", &pinned).unwrap().note.as_deref(), Some("pin: Tom asked for it here"), "the pin is the recorded override");

    let mut cloud = decision(cloud("a", "here"));
    cloud.here_cpu_held = Some(why.into());
    assert!(admit(&cloud, &launch, "", &local).unwrap_err().to_string().starts_with(why), "--local off a cloud placement meets the same line");
    assert_eq!(admit(&cloud, &launch, "", &pinned).unwrap().note.as_deref(), Some("local: needs this checkout; pin: Tom asked for it here"));
    assert!(admit(&cloud, &launch, "", &Overrides::default()).unwrap_err().to_string().contains("north cloud launch"), "a cloud box is sent to the cloud, not held");

    let mut elsewhere = decision(Target::Account("claude:a".into()));
    elsewhere.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: Some(1), accounts: vec!["a".into()], saturation: None });
    elsewhere.here_cpu_held = Some(why.into());
    assert_eq!(admit(&elsewhere, &launch, "", &Overrides::default()).unwrap().machine.map(|machine| machine.name), Some("far".to_owned()), "a box placed on another machine is not this host's load");
}

#[test]
fn a_launch_whose_named_machine_is_saturated_falls_through_to_the_next_open_machine_in_order_and_holds_only_when_none_is_open() {
    use crate::capacity::saturation::{Backlog, Saturation};
    let cfg: toml_edit::DocumentMut = "[placement]\norder = [\"gex45\", \"anthropic-cloud\", \"nexus\", \"laptop\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"native\"]\n[hosts.nexus]\nresident_cap = 8\nssh = \"tom@nexus\"\nserves = [\"native\"]\n[hosts.laptop]\nresident_cap = 8\nssh = \"tom@laptop\"\n".parse().unwrap();
    let reading = |load: f64| Saturation { memory_total_mib: 96000.0, memory_available_mib: 55000.0, disk_free_gib: 40.0, backlog: Some(Backlog { queued: 0, load, threads: 20 }) };
    let text = "Item: wisp#3 native window change\nSurface: rust\nNeeds: native\n";
    let launch = Launch { name: "wisp-3-d0250", account: "a", brief: None, cwd: Some(Path::new("/home/tom/code/wisp/main")), machine: Some("gex45") };
    let placed_for = |launch: &Launch, nexus_live: i64, nexus_accounts: &[&str], overrides: &Overrides| {
        let mut machines = placement::machines(&cfg);
        for machine in &mut machines {
            machine.live = Some(3);
            machine.accounts = vec!["a".into()];
            machine.saturation = Some(reading(4.0));
        }
        machines[0].saturation = Some(reading(39.0));
        machines[1].live = Some(nexus_live);
        machines[1].accounts = nexus_accounts.iter().map(|&label| label.to_owned()).collect();
        let mut decision = placement::decide(text, &cfg, vec![verdict("claude:a", Some(60.0))], (false, String::new()), &machines, "laptop", false);
        decision.run_on(machines[0].clone());
        fall_through(&mut decision, launch, overrides);
        admit(&decision, launch, text, overrides)
    };
    let placed = |nexus_live, nexus_accounts: &[&str], overrides: &Overrides| placed_for(&launch, nexus_live, nexus_accounts, overrides);

    let moved = placed(3, &["a"], &Overrides::default()).unwrap();
    assert_eq!(moved.machine.map(|machine| machine.name).as_deref(), Some("nexus"));
    assert_eq!(moved.note.as_deref(), Some("gex45 saturated; placed on nexus (gex45 skipped: load 39 on 20 threads)"));

    let full = placed(8, &["a"], &Overrides::default()).unwrap_err();
    assert_eq!(full, Refusal::Saturated("gex45 skipped: load 39 on 20 threads; launch it elsewhere, or keep it on that machine with --pin \"<reason>\", or here with --local \"<reason>\"".into()), "nexus full and the laptop serves no native box, so the launch waits");
    assert!(matches!(placed(3, &["c"], &Overrides::default()), Err(Refusal::Saturated(_))), "a machine not signed in to the launch's account never takes it");

    let pinned = placed(3, &["a"], &Overrides { pin: Some("Tom named gex45".into()), ..Overrides::default() }).unwrap();
    assert_eq!((pinned.machine.map(|machine| machine.name).as_deref(), pinned.note.as_deref()), (Some("gex45"), Some("pin: Tom named gex45")));
    let revived = placed_for(&Launch { cwd: None, ..launch }, 3, &["a"], &Overrides::default());
    assert!(matches!(revived, Err(Refusal::Saturated(_))), "a revive stays on the machine that holds its conversation: {revived:?}");
}
