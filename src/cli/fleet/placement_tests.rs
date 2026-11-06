use super::*;
use crate::pacing::placement::gate::{self as placement, Verdict};
use crate::pacing::placement::room::{PlanSize, Rank, Room};

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
        here_cpu_held: None,
        here_on_battery: None,
        crowded: Vec::new(),
        here_closed: None,
        open: Vec::new(),
        moved: None,
    }
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

