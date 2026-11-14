use super::placement::gate::{self, Decision, Machine, Target, Verdict};
use super::placement::room::Basis;
use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::usage::decide::world::ModelFamily;
use north_strategy_types::placement as record;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

const CODE_ONLY: &str = "Item: north#12 tidy placement\nSurface: rust\nNeeds: code\n";
const WARCRAFT: &str = "Item: makgora#4 LAN check\nSurface: rust\nNeeds: warcraft, native\n";

pub(super) fn control(five: (f64, &str), week: f64) -> Value {
    json!([
        {"window": "5h", "used_pct": five.0, "resets_at": five.1},
        {"window": "week", "used_pct": week, "resets_at": "2026-10-20T00:00:00Z"},
    ])
}

pub(super) fn gate(accounts: &[(&str, Value)]) -> Value {
    let controls: serde_json::Map<String, Value> = accounts.iter().map(|(n, c)| ((*n).to_owned(), c.clone())).collect();
    json!({"claude": {"windows": controls}})
}

fn roster(identities: &[(&str, Option<&str>)]) -> Roster {
    Roster::distinct(identities.iter().map(|(label, identity)| {
        let account = Account { provider: ProviderName::new("claude"), label: (*label).into(), home: format!("/profiles/claude-{label}").into(), token_file: None, api: None };
        (account, identity.map(str::to_owned))
    }), &Declared::default())
}

fn verdicts(quota: &Value, roster: &Roster) -> Vec<Verdict> {
    let at = crate::foundation::clock::rfc3339_seconds("2026-10-19T00:00:00Z").unwrap();
    gate::verdicts(quota, roster, &Basis::new(&DocumentMut::new(), at))
}

pub(super) fn decide(text: &str, cfg: &DocumentMut, quota: &Value, credit: (bool, String), machines: &[Machine], here: &str) -> Decision {
    let listed = roster(&[("a", None), ("b", None), ("c", None)]);
    gate::decide(text, cfg, verdicts(quota, &listed), credit, machines, here, false)
}

pub(super) fn target(decision: &super::placement::gate::Decision) -> String {
    match &decision.target {
        Target::Cloud(_) => "anthropic-cloud".into(),
        Target::Account(name) => name.clone(),
        Target::Held(why) => format!("held: {why}"),
        Target::Unserved(why) => format!("unserved: {why}"),
    }
}

#[test]
fn placement_never_picks_a_label_logged_into_another_labels_account() {
    let cfg: DocumentMut = "[hosts.here]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[
        ("claude:a", control((60.0, "2026-10-16T16:00:00Z"), 20.0)),
        ("claude:b", control((5.0, "2026-10-16T16:00:00Z"), 10.0)),
        ("claude:c", control((70.0, "2026-10-16T16:00:00Z"), 50.0)),
    ]);
    let shared = roster(&[("a", Some("one")), ("b", Some("one")), ("c", Some("two"))]);
    let decision = gate::decide(WARCRAFT, &cfg, verdicts(&quota, &shared), (false, String::new()), &[], "here", false);
    assert_eq!(target(&decision), "claude:a", "{:?}", decision.verdicts);
    let b = decision.verdict("claude:b").unwrap();
    assert!(!b.available && b.line.ends_with(&shared.duplicates[0].to_string()), "{}", b.line);
}

#[test]
fn needs_on_separate_lines_place_like_needs_on_one_line() {
    let cfg = "[hosts.here]\nssh = \"here\"\nserves = [\"private\", \"local-state\"]\n".parse().unwrap();
    let mut signed_in = gate::machines(&cfg);
    signed_in[0].claude_ai = vec!["c".into()];
    let gate = gate(&[("claude:c", control((10.0, "2026-10-16T14:00:00Z"), 30.0))]);
    let credit = || (true, "cloud credit: unread, expires in 19 days".to_owned());
    let place = |brief: &str| {
        let decision = decide(brief, &cfg, &gate, credit(), &signed_in, "here");
        (target(&decision), super::placement::gate::summary(&decision))
    };
    let head = "Item: north#12 tidy placement\nSurface: rust\n";
    let lines = place(&format!("{head}Needs: code\nNeeds: local-state\n"));
    assert_eq!(lines, place(&format!("{head}Needs: code, local-state\n")));
    assert_eq!(lines.0, "claude:c");
    assert_eq!(place(CODE_ONLY).0, "anthropic-cloud");
    let token_only = decide(CODE_ONLY, &cfg, &gate, credit(), &gate::machines(&cfg), "here");
    assert_eq!(target(&token_only), "claude:c", "on 2026-11-06 placement sent a code-only box to anthropic-cloud under an account signed in by token alone, where claude --cloud refused it (north#691)");
    let expired = decide(CODE_ONLY, &cfg, &gate, (false, "cloud credit: expired 2026-11-05T07:59Z".to_owned()), &[], "here");
    assert_eq!((target(&expired).as_str(), super::placement::gate::summary(&expired).contains("anthropic-cloud")), ("claude:c", false), "an expired credit placed code-only boxes on anthropic-cloud on 2026-11-06, where claude --cloud refused");
}

#[test]
fn placement_never_pairs_a_machine_with_a_label_not_signed_in_there() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nlaunches = true\nserves = [\"warcraft\", \"native\"]\n[hosts.laptop]\nserves = [\"warcraft\", \"native\"]\n".parse().unwrap();
    let quota = gate(&[
        ("claude:a", control((60.0, "2026-10-16T16:00:00Z"), 20.0)),
        ("claude:b", control((5.0, "2026-10-16T16:00:00Z"), 10.0)),
        ("claude:c", control((70.0, "2026-10-16T16:00:00Z"), 50.0)),
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
fn a_box_never_goes_to_a_host_that_cannot_push_its_repository() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\nprivate_repos = [\"north\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nlaunches = true\npushes = [\"north\", \"wisp\"]\n[hosts.laptop]\nresident_cap = 8\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, "2026-10-16T16:00:00Z"), 13.0))]);
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

    let slots: DocumentMut = "[hosts.gex45]\ngpu_slots = 4\n[hosts.laptop]\nserves = [\"native\"]\n".parse().unwrap();
    assert_eq!(gate::gpu_slots(&slots, "gex45"), Some(4));
    assert_eq!(gate::gpu_slots(&slots, "laptop"), None);
}

#[test]
fn a_needs_host_launch_runs_on_that_host_and_only_a_launch_host_with_room_takes_a_box() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\", \"ccx33\", \"nexus\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nlaunches = true\n[hosts.ccx33]\nresident_cap = 8\nssh = \"tom@ccx33\"\nlaunches = true\n[hosts.nexus]\nresident_cap = 6\nssh = \"tom@nexus\"\n[hosts.laptop]\nresident_cap = 8\nreaches = [\"gex45\"]\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, "2026-10-16T16:00:00Z"), 13.0))]);
    let credit = (false, "cloud credit: spent".to_owned());
    let brief = "Item: north#9 gex45 build\nSurface: rust\nNeeds: gex45\n";
    let machines = |gex45_live: i64| {
        let mut machines = gate::machines(&cfg);
        for machine in &mut machines {
            machine.live = Some(0);
            machine.accounts = vec!["c".into()];
        }
        machines[0].live = Some(gex45_live);
        machines
    };

    let placed = decide(brief, &cfg, &quota, credit.clone(), &machines(3), "laptop");
    assert_eq!(placed.machine.as_ref().map(|machine| machine.name.as_str()), Some("gex45"), "{:?}", placed.machine_lines);
    assert!(placed.machine_lines.contains(&"ccx33 skipped: cannot reach gex45".to_owned()), "{:?}", placed.machine_lines);
    assert!(placed.machine_lines.contains(&"nexus skipped: not a launch host".to_owned()), "{:?}", placed.machine_lines);
    assert_eq!(gate::lacks(&cfg, "ccx33", brief).as_deref(), Some("reach to gex45"), "north fleet dispatch refuses the same host fleet placement skips (north#654 ran twice on gex45 under Needs: whiterabbit)");
    assert_eq!(gate::lacks(&cfg, "gex45", brief), None);
    assert_eq!(gate::lacks(&cfg, "laptop", brief), None);

    let full = decide(brief, &cfg, &quota, credit.clone(), &machines(24), "laptop");
    assert_eq!(full.machine, None, "a host at its cap is never chosen: {:?}", full.machine_lines);
    assert!(full.machine_lines.contains(&"gex45 full: 24/24 resident CLIs".to_owned()), "{:?}", full.machine_lines);

    let code = "Item: north#9 tidy\nSurface: rust\nNeeds: code\n";
    let mut only_nexus = machines(24);
    only_nexus[1].live = Some(8);
    let held = decide(code, &cfg, &quota, credit, &only_nexus, "laptop");
    assert_eq!(held.machine, None, "nexus is not a launch host even with room: {:?}", held.machine_lines);
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\", \"ccx33\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nlaunches = true\nserves = [\"native:wisp\", \"client:wisp\"]\n[hosts.ccx33]\nresident_cap = 8\nssh = \"tom@ccx33\"\nlaunches = true\nserves = []\n[hosts.whiterabbit]\nresident_cap = 8\nreaches = [\"gex45\", \"ccx33\"]\nserves = [\"native\", \"sign-ins\", \"laptop\"]\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, "2026-10-16T16:00:00Z"), 13.0))]);
    let mut machines = gate::machines(&cfg);
    for machine in &mut machines {
        machine.live = Some(0);
        machine.accounts = vec!["c".into()];
    }
    for needs in ["laptop", "chrome-profile", "gpu", "native-client"] {
        let brief = format!("Item: north#706 laptop box\nSurface: rust\nNeeds: {needs}\n");
        let placed = decide(&brief, &cfg, &quota, (false, "cloud credit: spent".to_owned()), &machines, "whiterabbit");
        assert_eq!(placed.machine, None, "{needs}: {:?}", placed.machine_lines);
        for host in ["gex45", "ccx33"] {
            assert!(gate::lacks(&cfg, host, &brief).is_some(), "{needs} on {host}");
        }
        assert_eq!(gate::lacks(&cfg, "whiterabbit", &brief), None, "{needs}");
    }
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    const LOCAL_NEEDS: [&str; 12] = ["warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop"];

    fn local_need() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(LOCAL_NEEDS.to_vec())
    }

    proptest! {
        #[test]
        fn prop_a_box_with_a_local_need_is_never_placed_on_cloud(need in local_need(), on_surface in any::<bool>(), shout in any::<bool>(), five in 0.0..90.0f64) {
            let need = if shout { need.to_uppercase() } else { need.to_owned() };
            let text = if on_surface { format!("Item: north#12 tidy\nSurface: rust, {need}\nNeeds: code\n") } else { format!("Item: north#12 tidy\nSurface: rust\nNeeds: code, {need}\n") };
            let cfg: DocumentMut = "[hosts.here]\nserves = [\"warcraft\", \"wc3\", \"client\", \"native\", \"sign-ins\", \"private\", \"local-state\", \"tui\", \"fleet\", \"live\", \"desktop\", \"laptop\"]\n".parse().unwrap();
            let quota = gate(&[("claude:a", control((five, "2026-10-19T03:00:00Z"), 10.0))]);
            let decision = decide(&text, &cfg, &quota, (true, "cloud credit: unread".to_owned()), &[], "here");
            prop_assert!(!decision.code_only, "{}", decision.surface);
            prop_assert!(!matches!(decision.target, Target::Cloud(_)), "{:?}", decision.target);
        }
    }
}


#[test]
fn a_brief_with_no_surface_line_takes_its_repositorys_default_surfaces_and_a_replayed_case_decides_by_them() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nlaunches = true\n[hosts.laptop]\nresident_cap = 1\n".parse().unwrap();
    let quota = gate(&[("claude:c", control((6.0, "2026-10-16T16:00:00Z"), 13.0))]);
    let mut machines = gate::machines(&cfg);
    machines[0].live = Some(0);
    machines[0].accounts = vec!["c".into()];
    let brief = "Category: tooling\nRepository: north\n\nnorth#856: read the lander's journal.\n";

    let placed = decide(brief, &cfg, &quota, (false, "cloud credit: spent".to_owned()), &machines, "laptop");

    assert_eq!(placed.machine.as_ref().map(|machine| machine.name.as_str()), Some("gex45"), "{:?}", placed.machine_lines);
    assert_eq!(gate::surfaced(brief, &cfg).lines().next(), Some("Surface: rust"));
    let configured: DocumentMut = "[placement.surfaces]\nnorth = \"rust, tui\"\n".parse().unwrap();
    assert_eq!(gate::surfaced(brief, &configured).lines().next(), Some("Surface: rust, tui"));
    let surfaces: record::Config = serde_json::from_value(json!({
        "sha256": "",
        "order": [],
        "public_repos": [],
        "private_repos": [],
        "hosts": [],
        "plans": {},
        "surfaces": {"north": "rust, tui"},
    }))
    .unwrap();
    let case = record::Case {
        at: crate::foundation::clock::rfc3339_seconds("2026-10-16T12:00:00Z").unwrap(),
        launch: record::Launch::default(),
        brief: brief.into(),
        config: surfaces,
        credit: record::Credit { configured: true, ..Default::default() },
        usage: record::Usage::default(),
        roster: BTreeMap::new(),
        machines: Vec::new(),
        here: record::Here::default(),
        github: None,
        grants: Vec::new(),
        line_waits: Vec::new(),
    };
    let replay = super::placement::case::decide(&case).unwrap();
    assert_eq!((replay.code_only, replay.surface.as_str()), (false, "needs tui, not cloud"));

    let dropped = super::placement::case::decided_keys_lost();
    assert!(dropped.is_empty(), "the case document drops {dropped:?}, which decide() reads");
}

/// A replay of a coordinator launch on Fable with two accounts, each carrying its shared week and its
/// own Fable meter, exactly as the decision log records them.
fn coordinator_case(fable: [f64; 2], shared: [f64; 2]) -> record::Case {
    let at = crate::foundation::clock::rfc3339_seconds("2026-11-10T12:00:00Z").unwrap();
    let window = |name: &str, used: f64| record::Window { window: Some(name.into()), used_pct: Some(used), resets_at: None, reset_h: Some(100.0) };
    let account = |fable: f64, shared: f64| vec![window("5h", 0.0), window("week", shared), window("weekly_scoped/Fable", fable)];
    record::Case {
        at,
        launch: record::Launch { name: "proxy".into(), fresh: true, provider: "claude".into(), model: Some("fable".into()), role: Some("proxy".into()), ..Default::default() },
        brief: "Item: north#1065 coordinator wake\n".into(),
        config: record::Config::default(),
        credit: record::Credit::default(),
        usage: record::Usage { controls: BTreeMap::from([("claude:a".into(), account(fable[0], shared[0])), ("claude:b".into(), account(fable[1], shared[1]))]), ..Default::default() },
        roster: BTreeMap::from([("claude:a".into(), record::Standing::Listed), ("claude:b".into(), record::Standing::Listed)]),
        machines: Vec::new(),
        here: record::Here::default(),
        github: None,
        grants: Vec::new(),
        line_waits: Vec::new(),
    }
}

#[test]
fn a_replayed_coordinator_launch_reads_the_fable_meter_and_falls_back_to_opus_high_when_none_has_fable_headroom() {
    let upstairs = super::placement::case::decide(&coordinator_case([99.0, 40.0], [10.0, 10.0])).unwrap();
    assert_eq!((target(&upstairs).as_str(), upstairs.family), ("claude:b", Some(ModelFamily::Fable)), "{:?}", upstairs.verdicts);

    let all_fable_out = super::placement::case::decide(&coordinator_case([99.0, 99.0], [90.0, 10.0])).unwrap();
    assert_eq!((target(&all_fable_out).as_str(), all_fable_out.family), ("claude:b", Some(ModelFamily::Opus)), "{:?}", all_fable_out.verdicts);
}

#[test]
fn the_declared_operational_reserve_is_never_offered_to_a_worker_and_neither_an_account_at_99_percent_of_its_week_nor_a_small_plan_resetting_sooner_is_picked_beside_a_roomy_account() {
    let cfg: DocumentMut = "[accounts.claude]\nreserve = [\"b\"]\n".parse().unwrap();
    let at = crate::foundation::clock::rfc3339_seconds("2026-10-19T00:00:00Z").unwrap();
    let listed = roster(&[("a", None), ("b", None)]);
    let quota = gate(&[("claude:a", control((50.0, "2026-10-19T03:00:00Z"), 50.0)), ("claude:b", control((0.0, "2026-10-19T03:00:00Z"), 0.0))]);
    let decision = gate::decide(CODE_ONLY, &cfg, gate::verdicts(&quota, &listed, &Basis::new(&cfg, at)), (false, String::new()), &[], "here", false);
    let line = decision.verdict("claude:b").unwrap().line.clone();
    assert!(line.contains("unavailable") && line.contains("reserve: operational callers only"), "{line}");
    assert_eq!(target(&decision), "claude:a");

    let week = |used: f64, resets_at: &str| json!([{"window": "5h", "used_pct": 10.0, "resets_at": "2026-10-19T03:00:00Z"}, {"window": "week", "used_pct": used, "resets_at": resets_at}]);
    let quota = gate(&[("claude:a", week(99.0, "2026-10-19T02:00:00Z")), ("claude:b", week(41.0, "2026-10-20T00:00:00Z")), ("claude:c", week(29.0, "2026-10-22T00:00:00Z"))]);
    let listed = roster(&[("a", None), ("b", None), ("c", None)]);
    let spent = verdicts(&quota, &listed);
    assert!(spent.iter().any(|verdict| verdict.account == "claude:a" && verdict.available && verdict.headroom == 1.0), "{spent:?}");
    for brief in [CODE_ONLY, "Item: north#12 tidy placement\nNeeds: code\n"] {
        let decision = gate::decide(brief, &DocumentMut::new(), spent.clone(), (false, String::new()), &[], "here", false);
        let best = decision.best_account().map(|verdict| verdict.account.clone());
        assert!(matches!(best.as_deref(), Some("claude:b" | "claude:c")), "{best:?} {:?}", decision.verdicts);
        assert!(matches!(target(&decision).as_str(), "claude:b" | "claude:c"), "{}", target(&decision));
    }
    let only_spent = verdicts(&gate(&[("claude:a", week(99.0, "2026-10-19T02:00:00Z"))]), &listed);
    assert_eq!(target(&gate::decide(CODE_ONLY, &DocumentMut::new(), only_spent, (false, String::new()), &[], "here", false)), "claude:a");

    let plans: DocumentMut = "[accounts.claude]\nplans = { a = \"ultra-10x\", e = \"max-5x\" }\n".parse().unwrap();
    let roomy = json!([{"window": "week", "used_pct": 6.0, "resets_at": "2026-10-24T10:00:00Z"}]);
    let small = json!([{"window": "5h", "used_pct": 80.0, "resets_at": "2026-10-19T03:00:00Z"}, {"window": "week", "used_pct": 72.0, "resets_at": "2026-10-23T11:00:00Z"}]);
    let both = gate::verdicts(&gate(&[("claude:a", roomy), ("claude:e", small)]), &roster(&[("a", None), ("e", None)]), &Basis::new(&plans, at));
    let drawn = gate::decide(CODE_ONLY, &plans, both, (false, String::new()), &[], "here", false);
    assert_eq!(target(&drawn), "claude:a", "on 2026-11-13 dispatch drew the small plan e, resetting a day sooner, while a held 94% of a week twice e's size (north#259): {}", gate::summary(&drawn));
}

/// Opened by north#1054: the commit reviewer of 74aa9ca208 found that a grant with more than 2 dollars left counted as room above the subscription floor, so it was picked ahead of a plan account.
#[test]
fn no_api_organization_is_picked_ahead_of_a_plan_account_unless_the_owner_setting_allows_it() {
    let window = |name: &str, used: f64| record::Window { window: Some(name.into()), used_pct: Some(used), resets_at: None, reset_h: Some(100.0) };
    let grant = record::GrantSource { account: "claude:api:zenith".into(), left_usd: 70.0, deadline_h: 200.0, urgent: true, line: "$70 left".into() };
    let placed = |weeks: &[(&str, f64)], beside_plans: bool| {
        let case = record::Case {
            at: crate::foundation::clock::rfc3339_seconds("2026-11-13T12:00:00Z").unwrap(),
            launch: record::Launch { name: "worker".into(), fresh: true, provider: "claude".into(), ..Default::default() },
            brief: "Item: north#1054 place a worker\n".into(),
            config: record::Config { api_beside_plans: beside_plans, ..Default::default() },
            usage: record::Usage { controls: weeks.iter().map(|(account, used)| ((*account).into(), vec![window("5h", 0.0), window("week", *used)])).collect(), ..Default::default() },
            roster: weeks.iter().map(|(account, _)| ((*account).into(), record::Standing::Listed)).collect(),
            credit: record::Credit::default(),
            machines: Vec::new(),
            here: record::Here::default(),
            github: None,
            grants: vec![grant.clone()],
            line_waits: Vec::new(),
        };
        target(&super::placement::case::decide(&case).unwrap())
    };
    assert_eq!(placed(&[("claude:a", 99.0)], false), "claude:a", "a grant was picked ahead of a plan account at its week limit");
    assert_eq!(placed(&[("claude:a", 99.0), ("claude:b", 60.0)], false), "claude:b", "a grant the fleet cannot drain was picked ahead of an open plan account");
    assert_eq!(placed(&[("claude:a", 99.0), ("claude:b", 60.0)], true), "claude:api:zenith", "api_beside_plans ranks the grant by its deadline beside the plan accounts");
}

/// Opened by north#872: on 2026-11-07 `north fleet accounts --plan` printed claude:b as "5h 0%" with no five-hour sample.
#[test]
fn an_account_with_no_five_hour_sample_prints_its_five_hour_window_as_unread() {
    let week = json!([{"window": "week", "used_pct": 30.0, "resets_at": "2026-10-20T00:00:00Z"}]);
    let line = gate::verdict("claude:b", &week, None).line;
    assert_eq!(line, "claude:b available: 5h unread, week 30% (70% headroom)");
    let read = gate::verdict("claude:b", &control((10.0, "2026-10-19T03:00:00Z"), 30.0), None).line;
    assert_eq!(read, "claude:b available: 5h 10%, week 30% (70% headroom)");
}

fn waits(seconds: &[(&str, Option<u64>)]) -> Vec<record::LineWait> {
    seconds.iter().map(|(host, seconds)| record::LineWait { host: (*host).into(), seconds: *seconds }).collect()
}

fn launch_with(waits: Vec<record::LineWait>, role: &str, overrides: record::Overrides) -> gate::Decision {
    let mut case = coordinator_case([10.0, 10.0], [10.0, 10.0]);
    case.launch = record::Launch { name: "box".into(), role: Some(role.into()), overrides, model: None, ..case.launch };
    case.line_waits = waits;
    super::placement::case::decide(&case).unwrap()
}

/// Opened by north#1036: on 2026-11-13 morning launches were admitted while every build host's landing line was past its deadline; north#1362 added the excluded laptop.
#[test]
fn launch_waits_for_a_host_over_the_wait_bar_or_with_every_host_full_and_only_local_keeps_it_on_an_excluded_laptop() {
    let slow = waits(&[("cpx62", Some(212)), ("gex45", Some(95))]);
    let held = launch_with(slow.clone(), "worker", record::Overrides::default());
    let why = held.line_held.expect("every line is over 60 s");
    assert!(why.contains("over 60 s") && why.contains("95 s on gex45"), "{why}");

    let pinned = record::Overrides { pin: Some("urgent fix".into()), local: Some("needs the laptop".into()), ..Default::default() };
    assert!(launch_with(slow.clone(), "worker", pinned).line_held.is_some());

    assert!(launch_with(slow, "worker", record::Overrides { urgent: true, ..Default::default() }).line_held.is_some());

    assert_eq!(launch_with(waits(&[("cpx62", Some(212)), ("gex45", Some(60))]), "worker", record::Overrides::default()).line_held, None);
    assert_eq!(launch_with(waits(&[("cpx62", Some(212)), ("gex45", None)]), "worker", record::Overrides::default()).line_held, None);
    assert_eq!(launch_with(waits(&[("cpx62", Some(212))]), "lead", record::Overrides::default()).line_held, None);
    assert_eq!(launch_with(Vec::new(), "worker", record::Overrides::default()).line_held, None);

    let lane = || Target::Cloud(gate::CloudLane { account: "a".into(), host: "gex45".into(), credit: "credit".into() });
    let held_on_cloud = |local: Option<&str>| {
        let mut case = coordinator_case([10.0, 10.0], [10.0, 10.0]);
        case.launch = record::Launch { role: Some("worker".into()), overrides: record::Overrides { local: local.map(Into::into), ..Default::default() }, model: None, ..case.launch };
        case.line_waits = waits(&[("cpx62", Some(212))]);
        let mut decision = super::placement::case::decide(&case).unwrap();
        decision.line_held = None;
        decision.target = lane();
        decision.hold_for_line(&case);
        decision.line_held.is_some()
    };
    assert!(!held_on_cloud(None));
    assert!(held_on_cloud(Some("needs the laptop")));

    let full = record::Host { name: "gex45".into(), ssh: Some("tom@gex45".into()), launches: true, resident_cap: Some(1), ..Default::default() };
    let on_laptop = |local: Option<&str>| {
        let mut case = coordinator_case([10.0, 10.0], [10.0, 10.0]);
        case.launch = record::Launch { role: Some("worker".into()), overrides: record::Overrides { local: local.map(Into::into), ..Default::default() }, model: None, ..case.launch };
        case.brief = "Item: north#1362 eval\nSurface: rust\n".into();
        case.config = record::Config { hosts: vec![full.clone()], hosts_excluded: vec!["whiterabbit".into()], ..Default::default() };
        case.machines = vec![record::Machine { name: "gex45".into(), live: Some(1), accounts: vec!["a".into(), "b".into()], ..Default::default() }];
        case.here = record::Here { name: "whiterabbit".into(), ..Default::default() };
        super::placement::case::decide(&case).unwrap()
    };
    let waiting = on_laptop(None);
    assert!(waiting.held_for_room() && target(&waiting).contains("whiterabbit is in dispatch_hosts_excluded, so the launch waits for a host with room"), "on 2026-11-13 eval and sweep workers started on the excluded laptop with every host full (north#1362): {}", target(&waiting));
    let kept = on_laptop(Some("needs the laptop"));
    assert!(matches!(kept.target, Target::Account(_)) && kept.machine.is_none(), "{}", target(&kept));

    the_lines_waited_on_are_those_of_every_build_host_a_landing_goes_to_though_it_reaches_none_or_is_one_itself();
}

fn the_lines_waited_on_are_those_of_every_build_host_a_landing_goes_to_though_it_reaches_none_or_is_one_itself() {
    let cfg: DocumentMut = "[hosts.gex45]\nssh = \"tom@gex45\"\noffload_builds = true\n[hosts.cpx62]\nssh = \"tom@cpx62\"\noffload_builds = true\nreaches = []\n[hosts.nexus]\nssh = \"tom@nexus\"\n[hosts.whiterabbit]\nssh = \"tom@whiterabbit\"\nsettles_fleet = true\nreaches = [\"gex45\", \"cpx62\", \"nexus\"]\n".parse().unwrap();
    let names = |here: &str| super::placement::build_hosts(&cfg, here, gate::machines(&cfg)).into_iter().map(|machine| machine.name).collect::<Vec<_>>();
    assert_eq!(names("gex45"), ["gex45", "cpx62"]);
    assert_eq!(names("cpx62"), ["gex45", "cpx62"]);
    assert_eq!(names("whiterabbit"), ["gex45", "cpx62"]);
}

/// Opened by north#1036: a host closed for an eval block (`dispatch_hosts_excluded`) took a worker launched without --cwd, which voided the block.
#[test]
fn a_launch_placement_skips_a_host_dispatch_excludes_and_places_on_another_until_the_hold_lifts() {
    let placed = |excluded: Vec<String>| {
        let mut case = coordinator_case([10.0, 10.0], [10.0, 10.0]);
        case.launch = record::Launch { name: "worker".into(), role: Some("worker".into()), ..case.launch };
        case.brief = "Item: north#1036 fs-look\nSurface: rust\n".into();
        let host = |name: &str| record::Host { name: name.into(), ssh: Some(format!("tom@{name}")), launches: true, ..Default::default() };
        case.config = record::Config { hosts: vec![host("cpx62b"), host("gex45")], hosts_excluded: excluded, ..Default::default() };
        case.machines = ["cpx62b", "gex45"].into_iter().map(|name| record::Machine { name: name.into(), live: Some(0), accounts: vec!["a".into(), "b".into()], ..Default::default() }).collect();
        case.here = record::Here { name: "whiterabbit".into(), ..Default::default() };
        super::placement::case::decide(&case).unwrap()
    };
    let held = placed(vec!["cpx62b".into()]);
    assert!(held.machine_lines.contains(&"cpx62b skipped: dispatch_hosts_excluded names it".to_owned()), "{:?}", held.machine_lines);
    assert_eq!(held.machine.map(|machine| machine.name), Some("gex45".to_owned()), "{:?}", held.machine_lines);
    let lifted = placed(Vec::new());
    assert!(lifted.open.iter().any(|machine| machine.name == "cpx62b"), "{:?}", lifted.machine_lines);
}

#[test]
fn a_launch_needing_what_a_host_serves_its_operator_dials_the_operator_and_code_only_work_dials_its_agent_login() {
    let host = record::Host { name: "far".into(), ssh: Some("tom@far".into()), agent_ssh: Some("agent@far".into()), serves: vec!["native:wisp".into(), "private:makgora".into()], launches: true, ..Default::default() };
    let placed = |brief: &str, repository: &str, operator: bool| {
        let case = record::Case {
            launch: record::Launch { name: "worker".into(), fresh: true, provider: "claude".into(), named_machine: Some("far".into()), operator, repository: repository.into(), ..Default::default() },
            config: record::Config { hosts: vec![host.clone()], ..Default::default() },
            brief: brief.into(),
            ..coordinator_case([10.0, 10.0], [10.0, 10.0])
        };
        super::placement::case::decide(&case).unwrap().machine.map(|machine| machine.session_login().to_owned())
    };
    assert_eq!(placed("Item: wisp#4 window check\nSurface: rust\nNeeds: native\n", "wisp", true).as_deref(), Some("tom@far"), "a native box went to the agent login");
    assert_eq!(placed("Item: makgora#4 tidy\nSurface: rust\n", "makgora", true).as_deref(), Some("tom@far"), "a box on a repository whose private inputs the host serves went to the agent login");
    assert_eq!(placed("Item: north#12 tidy placement\nSurface: rust\n", "north", true).as_deref(), Some("agent@far"));
    assert_eq!(placed("Item: north#5 window check\nSurface: rust\nNeeds: native\n", "north", true).as_deref(), Some("agent@far"), "a need the host does not serve took the operator login");
    assert_eq!(placed("Item: wisp#4 window check\nSurface: rust\nNeeds: native\n", "wisp", false).as_deref(), Some("agent@far"), "a confined caller's native box took the operator login");
    assert_eq!(placed("Repository: wisp\nItem: north#5 window check\nSurface: rust\nNeeds: native\n", "north", true).as_deref(), Some("agent@far"), "a Repository: line in the brief moved a dispatched session onto the operator login");
}

#[test]
fn a_dispatched_brief_claiming_a_need_its_repository_config_lacks_stays_on_the_agent_login() {
    let host = record::Host { name: "far".into(), ssh: Some("tom@far".into()), agent_ssh: Some("agent@far".into()), serves: vec!["native".into()], launches: true, ..Default::default() };
    let placed = |surfaces: &[(&str, &str)]| {
        let case = record::Case {
            launch: record::Launch { name: "worker".into(), fresh: true, provider: "claude".into(), named_machine: Some("far".into()), operator: true, repository: "north".into(), ..Default::default() },
            config: record::Config { hosts: vec![host.clone()], surfaces: surfaces.iter().map(|(repo, surface)| ((*repo).to_owned(), (*surface).to_owned())).collect(), ..Default::default() },
            brief: "Item: north#5 window check\nSurface: native\nNeeds: native\n".into(),
            ..coordinator_case([10.0, 10.0], [10.0, 10.0])
        };
        super::placement::case::decide(&case).unwrap().machine.map(|machine| machine.session_login().to_owned())
    };
    assert_eq!(placed(&[]).as_deref(), Some("agent@far"), "a brief's own Needs: and Surface: words moved a dispatched session onto the operator login");
    assert_eq!(placed(&[("north", "native")]).as_deref(), Some("tom@far"), "a repository whose configured surface the host serves left its operator's work on the agent login");
}
