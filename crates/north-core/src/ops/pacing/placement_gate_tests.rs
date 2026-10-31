use super::placement::gate::{self, Decision, Machine, Target, Verdict};
use super::placement::room::Basis;
use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use serde_json::{Value, json};
use toml_edit::DocumentMut;

const CODE_ONLY: &str = "Item: north#12 tidy placement\nSurface: rust\nNeeds: code\n";
const WARCRAFT: &str = "Item: smashcraft#4 LAN check\nSurface: rust\nNeeds: warcraft, native\n";
const PRIVATE: &str = "Item: galileo#3 private change\nSurface: rust\n";
const UNDECLARED: &str = "Item: wisp#7 a box without facets\n";

fn control(five: (f64, f64, &str), week: (f64, f64)) -> Value {
    json!({"windows": [
        {"window": "5h", "used_pct": five.0, "burn_pct_h": five.1, "resets_at": five.2},
        {"window": "week", "used_pct": week.0, "burn_pct_h": week.1, "resets_at": "2026-10-20T00:00:00Z"},
    ]})
}

fn gate(accounts: &[(&str, Value)]) -> Value {
    let controls: serde_json::Map<String, Value> = accounts.iter().map(|(n, c)| ((*n).to_owned(), c.clone())).collect();
    json!({"claude": {"controls": controls}})
}

fn roster(identities: &[(&str, Option<&str>)]) -> Roster {
    Roster::distinct(identities.iter().map(|(label, identity)| {
        let account = Account { provider: ProviderName::new("claude"), label: (*label).into(), home: format!("/profiles/claude-{label}").into(), token_file: None };
        (account, identity.map(str::to_owned))
    }), &Declared::default())
}

fn verdicts(quota: &Value, roster: &Roster) -> Vec<Verdict> {
    let at = crate::pace::target::seconds("2026-10-19T00:00:00Z").unwrap();
    gate::verdicts(quota, roster, &Basis::new(&DocumentMut::new(), json!({}), at))
}

fn decide(text: &str, cfg: &DocumentMut, quota: &Value, credit: (bool, String), machines: &[Machine], here: &str) -> Decision {
    let listed = roster(&[("a", None), ("b", None), ("c", None)]);
    gate::decide(text, cfg, verdicts(quota, &listed), credit, machines, here, false)
}

fn target(decision: &super::placement::gate::Decision) -> String {
    match &decision.target {
        Target::Cloud(_) => "anthropic-cloud".into(),
        Target::Account(name) => name.clone(),
        Target::Held(why) => format!("held: {why}"),
        Target::Unserved(why) => format!("unserved: {why}"),
    }
}

#[test]
fn placement_ignores_the_pace_hold_and_refuses_only_the_five_hour_stop_burn_cap_and_provider_week() {
    let cfg = "[hosts.here]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let paced = |five: (f64, f64), week: f64, key: &str| {
        let mut c = control((five.0, five.1, "2026-10-16T16:00:00Z"), (week, 1.0));
        c[key] = json!(true);
        gate(&[("claude:b", c)])
    };
    let place = |quota: &Value| {
        let decision = decide(WARCRAFT, &cfg, quota, (false, "cloud credit: spent".to_owned()), &[], "here");
        (target(&decision), decision.verdict("claude:b").unwrap().line.clone())
    };
    for key in ["held", "over"] {
        let placed = place(&paced((40.0, 30.0), 60.0, key));
        assert_eq!(placed, ("claude:b".into(), "claude:b available: 5h 40%, week 60% (40% headroom)".into()), "{key}");
    }
    let refused = [
        ((20.0, 70.0), 60.0, "burning: 5h 70.0%/h > the 50%/h cap that keeps the window two hours"),
        ((96.0, 1.0), 60.0, "exhausted: 5h 96% >= 95%"),
        ((10.0, 1.0), 100.0, "exhausted: provider week 100%"),
    ];
    for (five, week, reason) in refused {
        let (placed, line) = place(&paced(five, week, "held"));
        assert_eq!(placed, "held: every Claude account exhausted or over its burn cap", "{line}");
        assert_eq!(line, format!("claude:b unavailable: {reason}"));
    }
}

#[test]
fn launch_placement_spends_the_credit_then_the_account_with_headroom_and_skips_a_burn_above_the_cap() {
    let cfg = "[placement]\nprivate_repos = [\"galileo\"]\n[hosts.here]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let credit = || (true, "cloud credit: unread, expires in 19 days".to_owned());
    let spent = || (false, "cloud credit: spent (read 2026-10-16), expires in 19 days".to_owned());
    let full_a = gate(&[
        ("claude:a", control((96.0, 5.0, "2026-10-16T14:00:00Z"), (40.0, 1.0))),
        ("claude:c", control((10.0, 5.0, "2026-10-16T14:00:00Z"), (30.0, 1.0))),
    ]);
    let burning_c = gate(&[
        ("claude:a", control((35.0, 10.0, "2026-10-16T16:00:00Z"), (50.0, 3.0))),
        ("claude:c", control((42.0, 86.0, "2026-10-16T16:00:00Z"), (20.0, 1.0))),
    ]);
    let pace_only = gate(&[("claude:c", control((80.0, 9.0, "2026-10-16T14:30:00Z"), (98.0, 0.5)))]);
    let cases = [
        ("code-only public box with credit", CODE_ONLY, &full_a, credit(), "anthropic-cloud"),
        ("code-only box once the credit is spent", CODE_ONLY, &full_a, spent(), "claude:c"),
        ("Warcraft box stays local", WARCRAFT, &full_a, credit(), "claude:c"),
        ("private repository stays local", PRIVATE, &burning_c, credit(), "claude:a"),
        ("undeclared surface stays local", UNDECLARED, &burning_c, credit(), "claude:a"),
        ("a 5h burn above the cap is skipped", WARCRAFT, &burning_c, credit(), "claude:a"),
        ("pace lines and a weekly projection never hold", WARCRAFT, &pace_only, credit(), "claude:c"),
        ("no live quota holds", WARCRAFT, &json!({}), credit(), "held: no live Claude quota in usage-gate.json; run pace-control"),
    ];
    for (name, brief, gate, credit, expected) in cases {
        let decision = decide(brief, &cfg, gate, credit, &[], "here");
        assert_eq!(target(&decision), expected, "{name}: {:?}", decision.verdicts);
        assert_eq!(decision.verdicts.len(), gate["claude"]["controls"].as_object().map_or(0, |c| c.len()), "{name}");
    }
    let decision = decide(WARCRAFT, &cfg, &burning_c, credit(), &[], "here");
    let c = decision.verdict("claude:c").unwrap();
    assert!(!c.available && c.line.contains("burning: 5h 86.0%/h"), "{}", c.line);
    let a = decide(WARCRAFT, &cfg, &full_a, credit(), &[], "here").verdict("claude:a").cloned().unwrap();
    assert!(!a.available && a.line.contains("exhausted: 5h 96% >= 95%"), "{}", a.line);
    assert!(decision.verdicts.iter().all(|v| !v.line.contains('\n') && v.line.starts_with(&v.account)));

    let cfg: toml_edit::DocumentMut = "[placement]\norder = [\"near\", \"far\"]\nprivate_repos = [\"galileo\"]\n[hosts.near]\nresident_cap = 2\nssh = \"tom@near\"\n[hosts.far]\nresident_cap = 4\nssh = \"tom@far\"\nserves = [\"private\", \"native\"]\n[hosts.nossh]\nresident_cap = 9\n[hosts.here]\nserves = [\"warcraft\", \"native\", \"private:smashcraft\"]\n".parse().unwrap();
    let spent = || (false, "cloud credit: spent".to_owned());
    let gate = gate(&[("claude:a", control((35.0, 1.0, "2026-10-16T16:00:00Z"), (50.0, 1.0)))]);
    let probed = |near: Option<i64>, far: Option<i64>| {
        let mut machines = super::placement::gate::machines(&cfg);
        assert_eq!(machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["near", "far"], "a host without ssh is no machine");
        machines[0].live = near;
        machines[1].live = far;
        for machine in &mut machines {
            machine.accounts = vec!["a".into(), "b".into(), "c".into()];
        }
        machines
    };
    let place = |brief: &str, near, far| {
        let decision = decide(brief, &cfg, &gate, spent(), &probed(near, far), "here");
        let summary = super::placement::gate::summary(&decision);
        (decision.machine.map(|m| m.ssh), decision.machine_lines, summary)
    };
    let native = "Item: smashcraft#9 render\nSurface: lua\nVerify: native\nNeeds: private\n";
    let cases = [
        ("code-only box takes the first machine with room", PRIVATE, Some(0), Some(0), Some("tom@near")),
        ("a full machine is passed over", PRIVATE, Some(2), Some(3), Some("tom@far")),
        ("an unreachable machine is passed over", PRIVATE, None, Some(0), Some("tom@far")),
        ("a native box goes only where native is served", native, Some(0), Some(0), Some("tom@far")),
        ("a Warcraft box nobody serves stays here", WARCRAFT, Some(0), Some(0), None),
        ("an undeclared box stays here", UNDECLARED, Some(0), Some(0), None),
    ];
    for (name, brief, near, far, expected) in cases {
        let (machine, lines, _) = place(brief, near, far);
        assert_eq!(machine.as_deref(), expected, "{name}: {lines:?}");
        assert_eq!(lines.len(), 2, "{name}: one line per machine");
    }
    let (_, lines, summary) = place(native, Some(0), None);
    assert_eq!(lines, ["near skipped: needs native", "far unreachable over ssh tom@far"]);
    assert_eq!(summary, "target here claude:a (needs native, not cloud)");
    let (_, _, summary) = place(PRIVATE, Some(0), Some(0));
    assert_eq!(summary, "target near claude:a (galileo is not a public repository, not cloud)");
}

#[test]
fn a_private_box_goes_only_to_a_host_that_holds_its_repository_store() {
    let cfg: toml_edit::DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"private:wisp\"]\n[hosts.laptop]\nserves = [\"private:smashcraft\", \"native\"]\n".parse().unwrap();
    let gate = gate(&[("claude:c", control((6.0, 1.0, "2026-10-16T16:00:00Z"), (13.0, 1.0)))]);
    let mut machines = super::placement::gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["c".into()];
    let spent = || (false, "cloud credit: spent".to_owned());
    let camera = "Item: smashcraft#96 (scorecard tool)\nSurface: ts\nVerify: visual\nNeeds: private\n";
    let wisp = "Item: wisp#5 private pool change\nSurface: ts\nNeeds: private\n";

    let placed = decide(camera, &cfg, &gate, spent(), &machines, "laptop");
    assert_eq!(placed.machine, None, "{:?}", placed.machine_lines);
    assert_eq!(placed.machine_lines, ["gex45 skipped: needs private"]);
    assert_eq!(super::placement::gate::summary(&placed), "target laptop claude:c (needs private, not cloud)");

    let mut placed = decide(wisp, &cfg, &gate, spent(), &machines, "laptop");
    assert_eq!(placed.machine.as_ref().map(|m| m.name.as_str()), Some("gex45"), "{:?}", placed.machine_lines);
    placed.stay_here(Some("gex45 has no /lane, so here".into()));
    assert_eq!(placed.machine_lines[1..], ["gex45 has no /lane, so here", "laptop skipped: needs private"]);
    assert_eq!(super::placement::gate::summary(&placed), "target unserved: no host serves private for wisp");

    let refused = decide(camera, &cfg, &gate, spent(), &[], "gex45");
    assert_eq!(refused.machine_lines, ["gex45 skipped: needs private"]);
    assert_eq!(super::placement::gate::summary(&refused), "target unserved: no host serves private for smashcraft");
}

#[test]
fn placement_never_picks_a_label_logged_into_another_labels_account() {
    let cfg: DocumentMut = "[hosts.here]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[
        ("claude:a", control((60.0, 1.0, "2026-10-16T16:00:00Z"), (20.0, 1.0))),
        ("claude:b", control((5.0, 1.0, "2026-10-16T16:00:00Z"), (10.0, 1.0))),
        ("claude:c", control((70.0, 1.0, "2026-10-16T16:00:00Z"), (50.0, 1.0))),
    ]);
    let shared = roster(&[("a", Some("one")), ("b", Some("one")), ("c", Some("two"))]);
    let decision = gate::decide(WARCRAFT, &cfg, verdicts(&quota, &shared), (false, String::new()), &[], "here", false);
    assert_eq!(target(&decision), "claude:a", "{:?}", decision.verdicts);
    let b = decision.verdict("claude:b").unwrap();
    assert!(!b.available && b.line.ends_with(&shared.duplicates[0].to_string()), "{}", b.line);
}

#[test]
fn needs_on_separate_lines_place_like_needs_on_one_line() {
    let cfg = "[hosts.here]\nserves = [\"private\", \"local-state\"]\n".parse().unwrap();
    let gate = gate(&[("claude:c", control((10.0, 5.0, "2026-10-16T14:00:00Z"), (30.0, 1.0)))]);
    let credit = || (true, "cloud credit: unread, expires in 19 days".to_owned());
    let place = |brief: &str| {
        let decision = decide(brief, &cfg, &gate, credit(), &[], "here");
        (target(&decision), super::placement::gate::summary(&decision))
    };
    let head = "Item: north#12 tidy placement\nSurface: rust\n";
    let lines = place(&format!("{head}Needs: code\nNeeds: local-state\n"));
    assert_eq!(lines, place(&format!("{head}Needs: code, local-state\n")));
    assert_eq!(lines.0, "claude:c");
    assert_eq!(place(CODE_ONLY).0, "anthropic-cloud");
}

#[test]
fn placement_never_pairs_a_machine_with_a_label_not_signed_in_there() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"warcraft\", \"native\"]\n[hosts.laptop]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[
        ("claude:a", control((60.0, 1.0, "2026-10-16T16:00:00Z"), (20.0, 1.0))),
        ("claude:b", control((5.0, 1.0, "2026-10-16T16:00:00Z"), (10.0, 1.0))),
        ("claude:c", control((70.0, 1.0, "2026-10-16T16:00:00Z"), (50.0, 1.0))),
    ]);
    let mut machines = super::placement::gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["a".into(), "c".into()];
    let switched = decide(WARCRAFT, &cfg, &quota, (false, String::new()), &machines, "laptop");
    assert_eq!((switched.machine.as_ref().map(|m| m.name.as_str()), target(&switched).as_str()), (Some("gex45"), "claude:a"), "{:?}", switched.machine_lines);
    assert!(switched.machine_lines.contains(&"gex45 has no signed-in claude:b; claude:a there".to_owned()), "{:?}", switched.machine_lines);

    machines[0].accounts = Vec::new();
    let kept = decide(WARCRAFT, &cfg, &quota, (false, String::new()), &machines, "laptop");
    assert_eq!((kept.machine.as_ref(), target(&kept).as_str()), (None, "claude:b"), "{:?}", kept.machine_lines);
    assert!(kept.machine_lines.contains(&"gex45 has no signed-in claude:b, so here".to_owned()), "{:?}", kept.machine_lines);
}

#[test]
fn a_box_declares_every_local_need_with_its_repository_as_its_capacity_need() {
    assert_eq!(gate::capacity_need(WARCRAFT).as_deref(), Some("native:smashcraft,warcraft:smashcraft"));
    assert_eq!(gate::capacity_need(CODE_ONLY), None);
}

#[test]
fn a_host_entry_names_the_warcraft_client_count_admission_holds_it_to() {
    let cfg: DocumentMut = "[hosts.gex45]\nresident_cap = 24\nwarcraft_clients = 6\n[hosts.laptop]\nserves = [\"native\"]\n".parse().unwrap();
    assert_eq!(gate::warcraft_clients(&cfg, "gex45"), Some(6));
    assert_eq!(gate::warcraft_clients(&cfg, "laptop"), None);
    assert_eq!(gate::warcraft_clients(&cfg, "elsewhere"), None);
}

#[test]
fn a_host_entry_names_the_bar1_reserve_admission_keeps_free() {
    let cfg: DocumentMut = "[hosts.gex45]\nwarcraft_clients = 6\nbar1_margin_mib = 96\n[hosts.laptop]\nserves = [\"native\"]\n".parse().unwrap();
    assert_eq!(gate::bar1_margin_mib(&cfg, "gex45"), Some(96));
    assert_eq!(gate::bar1_margin_mib(&cfg, "laptop"), None);
}

#[test]
fn placement_puts_a_label_owned_by_another_machine_only_on_that_machine() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"warcraft\", \"native\"]\n[hosts.laptop]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[
        ("claude:a", control((60.0, 1.0, "2026-10-16T16:00:00Z"), (50.0, 1.0))),
        ("claude:b", control((5.0, 1.0, "2026-10-16T16:00:00Z"), (10.0, 1.0))),
    ]);
    let found = ["a", "b"].map(|label| (Account { provider: ProviderName::new("claude"), label: label.into(), home: format!("/profiles/claude-{label}").into(), token_file: None }, None));
    let owned = Roster::distinct(found, &Declared { elsewhere: [("b".to_owned(), vec!["gex45".to_owned()])].into(), ..Declared::default() });
    let mut machines = super::placement::gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["b".into()];

    let there = gate::decide(WARCRAFT, &cfg, verdicts(&quota, &owned), (false, String::new()), &machines, "laptop", false);
    assert_eq!((there.machine.as_ref().map(|m| m.name.as_str()), target(&there).as_str()), (Some("gex45"), "claude:b"), "{:?}", there.machine_lines);

    let here = gate::decide(WARCRAFT, &cfg, verdicts(&quota, &owned), (false, String::new()), &[], "laptop", false);
    assert_eq!((here.machine.as_ref(), target(&here).as_str()), (None, "claude:a"), "{:?}", here.machine_lines);
    assert!(here.machine_lines.contains(&"claude:b runs only on gex45; claude:a here".to_owned()), "{:?}", here.machine_lines);
}

#[test]
fn the_probe_runs_on_the_roomiest_account_and_none_when_every_account_is_out() {
    let listed = roster(&[("b", None), ("c", None)]);
    let five = (10.0, 1.0, "2026-10-27T12:00:00Z");
    let week_out = control(five, (100.0, 0.0));
    let one_open = gate(&[("claude:b", week_out.clone()), ("claude:c", control(five, (40.0, 1.0)))]);
    assert_eq!(gate::roomiest_here(&one_open, &listed), Some("c".to_owned()));

    let all_out = gate(&[("claude:b", week_out.clone()), ("claude:c", week_out)]);
    assert_eq!(gate::roomiest_here(&all_out, &listed), None);
}

#[test]
fn the_probe_runs_on_an_account_whose_burn_refuses_a_launch_but_not_one_whose_week_is_out() {
    let listed = roster(&[("a", None), ("b", None), ("c", None)]);
    let burning = control((44.0, 191.0, "2026-10-27T12:00:00Z"), (36.0, 28.0));
    let week_out = control((3.0, 0.0, "2026-10-27T12:00:00Z"), (100.0, 0.0));
    let quota = gate(&[("claude:a", week_out.clone()), ("claude:b", week_out), ("claude:c", burning)]);
    assert_eq!(gate::roomiest_here(&quota, &listed), Some("c".to_owned()));
    assert!(!verdicts(&quota, &listed).iter().any(|verdict| verdict.available), "a launch is still refused");
}

#[test]
fn a_box_without_a_surface_leaves_a_host_in_game_reserve_for_one_serving_its_repositorys_native_and_private_work() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"native:smashcraft\", \"private:smashcraft\"]\n[hosts.laptop]\nserves = [\"private\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, 1.0, "2026-10-16T16:00:00Z"), (13.0, 1.0)))]);
    let mut machines = gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["c".into()];
    let listed = roster(&[("c", None)]);
    let placed = |text: &str, gaming: bool| gate::summary(&gate::decide(text, &cfg, verdicts(&quota, &listed), (false, "cloud credit: spent".to_owned()), &machines, "laptop", gaming));
    let smashcraft = "Item: smashcraft#9 Frostbolt startup\n";
    assert_eq!(placed(smashcraft, false), "target laptop claude:c (no Surface: declared, not cloud)");
    assert_eq!(placed(smashcraft, true), "target gex45 claude:c (no Surface: declared, not cloud)");
    assert_eq!(placed(UNDECLARED, true), "target laptop claude:c (no Surface: declared, not cloud)");
}

#[test]
fn a_box_never_goes_to_a_host_that_cannot_push_its_repository() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\npushes = [\"north\", \"wisp\"]\n[hosts.laptop]\nresident_cap = 8\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, 1.0, "2026-10-16T16:00:00Z"), (13.0, 1.0)))]);
    let mut machines = gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["c".into()];
    let south = "Item: south#9 timer change\nSurface: rust\nNeeds: code\n";
    let north = "Item: north#9 timer change\nSurface: rust\nNeeds: code\n";
    let credit = (false, "cloud credit: spent".to_owned());

    let kept = decide(south, &cfg, &quota, credit.clone(), &machines, "laptop");
    assert_eq!(kept.machine, None, "{:?}", kept.machine_lines);
    assert!(kept.machine_lines.contains(&"gex45 skipped: cannot push south".to_owned()), "{:?}", kept.machine_lines);
    assert_eq!(gate::lacks(&cfg, "gex45", south).as_deref(), Some("push to south"));
    assert_eq!(gate::lacks(&cfg, "laptop", south), None, "a host naming no list pushes any repository");

    let sent = decide(north, &cfg, &quota, credit, &machines, "laptop");
    assert_eq!(sent.machine.as_ref().map(|machine| machine.name.as_str()), Some("gex45"), "{:?}", sent.machine_lines);
    assert_eq!(gate::lacks(&cfg, "gex45", north), None);
}

#[test]
fn a_saturated_machine_is_skipped_by_name_and_the_next_one_takes_the_box() {
    use crate::capacity::saturation::Saturation;
    let cfg: toml_edit::DocumentMut = "[placement]\norder = [\"gex45\", \"nexus\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"private\"]\n[hosts.nexus]\nresident_cap = 8\nssh = \"tom@nexus\"\nserves = [\"private\"]\n[hosts.here]\n".parse().unwrap();
    let quota = gate(&[("claude:a", control((35.0, 1.0, "2026-10-16T16:00:00Z"), (50.0, 1.0)))]);
    let calm = Saturation { memory_total_mib: 96000.0, memory_available_mib: 55000.0, disk_free_gib: 40.0 };
    let place = |gex45: Saturation| {
        let mut machines = gate::machines(&cfg);
        for machine in &mut machines {
            machine.live = Some(3);
            machine.accounts = vec!["a".into()];
            machine.saturation = Some(calm);
        }
        machines[0].saturation = Some(gex45);
        let decision = decide(PRIVATE, &cfg, &quota, (false, String::new()), &machines, "here");
        (decision.machine.map(|machine| machine.name), decision.machine_lines)
    };
    assert_eq!(place(calm).0.as_deref(), Some("gex45"));
    let cases = [
        ("memory", Saturation { memory_available_mib: 5000.0, ..calm }, "gex45 skipped: 4.9 GiB of memory available, too little beside a 0.8 GiB session"),
        ("disk", Saturation { disk_free_gib: 14.0, ..calm }, "gex45 skipped: 14.0 GiB free on the root disk, under 15"),
    ];
    for (signal, reading, line) in cases {
        let (machine, lines) = place(reading);
        assert_eq!(machine.as_deref(), Some("nexus"), "{signal}: {lines:?}");
        assert_eq!(lines, [line, "nexus available: 3/8 resident CLIs"], "{signal}");
    }
}

#[test]
fn placement_picks_the_week_that_expires_soonest_then_a_live_spend_target_ahead_of_the_roomiest_account() {
    let cfg: DocumentMut = "[hosts.here]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let usage = |five: f64, week: f64, resets: &str| json!({"windows": [{"window": "5h", "used_pct": five, "resets_at": "2026-10-19T04:00:00Z"}, {"window": "week", "used_pct": week, "resets_at": resets}]});
    let quota = gate(&[("claude:a", usage(50.0, 70.0, "2026-10-19T10:00:00Z")), ("claude:b", usage(20.0, 30.0, "2026-10-26T00:00:00Z"))]);
    let listed = roster(&[("a", None), ("b", None)]);
    let at = crate::pace::target::seconds("2026-10-19T00:00:00Z").unwrap();
    let picked = |pace: Value| {
        let verdicts = gate::verdicts(&quota, &listed, &Basis::new(&DocumentMut::new(), pace, at));
        target(&gate::decide(CODE_ONLY, &cfg, verdicts, (false, String::new()), &[], "here", false))
    };
    assert_eq!(picked(json!({})), "claude:a", "a holds less headroom than b but its week expires sooner");
    let spending = json!({"accounts": {"claude:b": {"target": {"by": "2026-10-19T12:00:00Z"}}}});
    assert_eq!(picked(spending), "claude:b", "a live spend target outranks every room");
}

fn gex45() -> Machine {
    Machine { name: "gex45".into(), ssh: "gex45".into(), cap: 8, live: Some(0), serves: vec!["native:wisp".into(), "native:smashcraft".into()], pushes: None, accounts: vec!["a".into()], saturation: None }
}

fn placed_on(text: &str) -> Option<String> {
    let cfg = DocumentMut::new();
    let quota = gate(&[("claude:a", control((10.0, 1.0, "2026-10-19T04:00:00Z"), (10.0, 1.0)))]);
    decide(text, &cfg, &quota, (false, String::new()), &[gex45()], "laptop").machine.map(|machine| machine.name)
}

#[test]
fn a_prose_item_takes_its_repository_from_the_launch_directory_then_the_task() {
    let prose = "Item: The native window's GPU tests fail\nNeeds: native\nSurface: lua\n";
    assert_eq!(placed_on(prose), None);
    let by_directory = gate::anchored(prose, Some(std::path::Path::new("/home/tom/code/wisp/main")), None);
    assert_eq!(gate::repository(&by_directory), "wisp");
    assert_eq!(placed_on(&by_directory).as_deref(), Some("gex45"));
    let in_lane = gate::anchored(prose, Some(std::path::Path::new("/home/tom/code/smashcraft/worktrees/x/ts")), None);
    assert_eq!(gate::repository(&in_lane), "smashcraft");
    let by_task = gate::anchored(prose, Some(std::path::Path::new("/tmp/scratch")), Some("tompassarelli/smashcraft#518"));
    assert_eq!(gate::repository(&by_task), "smashcraft");
    assert_eq!(placed_on(&by_task).as_deref(), Some("gex45"));
}

#[test]
fn a_declared_repository_or_an_issue_reference_outranks_the_launch_directory() {
    let cwd = Some(std::path::Path::new("/home/tom/code/wisp/main"));
    let declared = "Repository: smashcraft\nItem: prose\nNeeds: native\n";
    assert_eq!(gate::repository(&gate::anchored(declared, cwd, None)), "smashcraft");
    let referenced = "Item: north#12 tidy\nNeeds: native\n";
    assert_eq!(gate::repository(&gate::anchored(referenced, cwd, Some("wisp#3"))), "north");
    assert_eq!(gate::anchored("Item: prose\n", Some(std::path::Path::new("/tmp")), None), "Item: prose\n");
}
