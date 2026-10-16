use super::placement::gate::{Target, decide};
use serde_json::{Value, json};

const AT: &str = "2026-10-16T12:00:00Z";
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

fn target(decision: &super::placement::gate::Decision) -> String {
    match &decision.target {
        Target::Cloud(_) => "anthropic-cloud".into(),
        Target::Account(name) => name.clone(),
        Target::Held(why) => format!("held: {why}"),
    }
}

#[test]
fn launch_placement_spends_the_credit_then_the_account_with_headroom_and_skips_runaway_burn() {
    let cfg = "[placement]\nprivate_repos = [\"galileo\"]\n".parse().unwrap();
    let at = super::target::seconds(AT).unwrap();
    let credit = || (true, "cloud credit: unread, expires in 19 days".to_owned());
    let spent = || (false, "cloud credit: spent (read 2026-10-16), expires in 19 days".to_owned());
    let full_a = gate(&[
        ("claude:a", control((96.0, 5.0, "2026-10-16T14:00:00Z"), (40.0, 1.0))),
        ("claude:c", control((10.0, 5.0, "2026-10-16T14:00:00Z"), (30.0, 1.0))),
    ]);
    let runaway_c = gate(&[
        ("claude:a", control((35.0, 10.0, "2026-10-16T16:00:00Z"), (50.0, 3.0))),
        ("claude:c", control((42.0, 86.0, "2026-10-16T16:00:00Z"), (20.0, 1.0))),
    ]);
    let pace_only = gate(&[("claude:c", control((80.0, 9.0, "2026-10-16T14:30:00Z"), (98.0, 0.5)))]);
    let cases: [(&str, &str, &Value, (bool, String), &str); 8] = [
        ("code-only public box with credit", CODE_ONLY, &full_a, credit(), "anthropic-cloud"),
        ("code-only box once the credit is spent", CODE_ONLY, &full_a, spent(), "claude:c"),
        ("Warcraft box stays local", WARCRAFT, &full_a, credit(), "claude:c"),
        ("private repository stays local", PRIVATE, &runaway_c, credit(), "claude:a"),
        ("undeclared surface stays local", UNDECLARED, &runaway_c, credit(), "claude:a"),
        ("runaway 5h burn is skipped", WARCRAFT, &runaway_c, credit(), "claude:a"),
        ("pace lines and a weekly projection never hold", WARCRAFT, &pace_only, credit(), "claude:c"),
        ("no live quota holds", WARCRAFT, &json!({}), credit(), "held: no live Claude quota in usage-gate.json; run pace-control"),
    ];
    for (name, brief, gate, credit, expected) in cases {
        let decision = decide(brief, &cfg, gate, credit, at);
        assert_eq!(target(&decision), expected, "{name}: {:?}", decision.verdicts);
        assert_eq!(decision.verdicts.len(), gate["claude"]["controls"].as_object().map_or(0, |c| c.len()), "{name}");
    }
    let decision = decide(WARCRAFT, &cfg, &runaway_c, credit(), at);
    let c = decision.verdict("claude:c").unwrap();
    assert!(!c.available && c.line.contains("runaway: 5h 42%"), "{}", c.line);
    let a = decide(WARCRAFT, &cfg, &full_a, credit(), at).verdict("claude:a").cloned().unwrap();
    assert!(!a.available && a.line.contains("exhausted: 5h 96% >= 95%"), "{}", a.line);
    assert!(decision.verdicts.iter().all(|v| !v.line.contains('\n') && v.line.starts_with(&v.account)));
}
