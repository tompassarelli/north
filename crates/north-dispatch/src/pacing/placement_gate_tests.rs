use super::placement::gate::{self, Decision, Machine, Target, Verdict};
use super::placement::room::Basis;
use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use serde_json::{Value, json};
use toml_edit::DocumentMut;

const CODE_ONLY: &str = "Item: north#12 tidy placement\nSurface: rust\nNeeds: code\n";
const WARCRAFT: &str = "Item: makgora#4 LAN check\nSurface: rust\nNeeds: warcraft, native\n";

pub(super) fn control(five: (f64, f64, &str), week: (f64, f64)) -> Value {
    json!({"windows": [
        {"window": "5h", "used_pct": five.0, "burn_pct_h": five.1, "resets_at": five.2},
        {"window": "week", "used_pct": week.0, "burn_pct_h": week.1, "resets_at": "2026-10-20T00:00:00Z"},
    ]})
}

pub(super) fn gate(accounts: &[(&str, Value)]) -> Value {
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
fn a_box_never_goes_to_a_host_that_cannot_push_its_repository() {
    let cfg: DocumentMut = "[placement]\norder = [\"gex45\"]\nprivate_repos = [\"north\"]\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\npushes = [\"north\", \"wisp\"]\n[hosts.laptop]\nresident_cap = 8\n".parse().unwrap();
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
            let quota = gate(&[("claude:a", control((five, 1.0, "2026-10-19T03:00:00Z"), (10.0, 1.0)))]);
            let decision = decide(&text, &cfg, &quota, (true, "cloud credit: unread".to_owned()), &[], "here");
            prop_assert!(!decision.code_only, "{}", decision.surface);
            prop_assert!(!matches!(decision.target, Target::Cloud(_)), "{:?}", decision.target);
        }
    }
}

