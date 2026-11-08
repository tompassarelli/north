use super::billing;
use super::render;
use super::Screen;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{AccountOutlook, Runway};

const MANIFEST: &str = r#"
[[machine]]
id = "whiterabbit"
monthly_usd = 0.0

[[machine]]
id = "nexus"
monthly_usd = 48.0

[[machine]]
id = "cpx62"
hourly_eur = 0.2452
monthly_cap_eur = 152.99
monthly_usd = 171.71

[[service]]
id = "anthropic-a"
plan = "Ultra"
monthly_usd = 216.67

[[service]]
id = "github"
plan = "free"
monthly_usd = "unread"
"#;

fn account(name: &str) -> AccountOutlook {
    AccountOutlook { account: name.to_owned(), week_used: Some(82.0), week_resets: Some(Timestamp::from_unix_seconds(1_794_000_000)), burn_per_hour: Some(5.8), runway: Runway::Empties { minutes: 188.0 } }
}

#[test]
fn the_screen_names_each_plan_and_machine_within_fifty_lines_of_120_characters_and_colours_only_on_request() {
    let screen = Screen {
        window: "24h".to_owned(),
        now: 1_793_990_000,
        bars: Err("unread".to_owned()),
        accounts: Ok(["a", "b", "c", "d"].map(account).to_vec()),
        runs: Err("unread".to_owned()),
        attempts: Err("unread".to_owned()),
        week_runs: Err("unread".to_owned()),
        billing: billing::parse(MANIFEST),
    };
    let text = render::screen(&screen, false);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() <= 50, "{} lines", lines.len());
    assert!(!text.contains('\x1b'));
    assert!(render::screen(&screen, true).contains("\x1b[3"));
    let start = lines.iter().position(|line| line.starts_with("Billing:")).expect("a billing section");
    let end = lines[start..].iter().position(|line| line.is_empty()).map_or(lines.len(), |gap| start + gap);
    assert!(lines.iter().all(|line| line.chars().count() <= 120), "{text}");
    let section = lines[start..end].join("\n");
    assert!(section.contains("Ultra $216.67/month"), "{section}");
    assert!(section.contains("€0.2452/h, capped at €152.99/month"), "{section}");
    assert!(section.contains("up to $219.71"), "{section}");
}

fn run(account: &str, mix: (&str, &str), started: &str, usd: f64) -> crate::ledger::WindowRun {
    crate::ledger::WindowRun { agent: "x".to_owned(), role: "worker".to_owned(), category: "tooling".to_owned(), tier: String::new(), model: mix.0.to_owned(), effort: mix.1.to_owned(), account: account.to_owned(), started: started.to_owned(), usd }
}

#[test]
fn a_class_uses_the_share_of_a_week_its_dollars_are_of_the_week_sized_from_the_spend_so_far_and_api_credit_runs_stay_apart() {
    let resets = Timestamp::from_unix_seconds(1_794_000_000);
    let in_week = Timestamp::from_unix_seconds(1_794_000_000 - 86_400).iso();
    let week_runs = vec![run("claude-b", ("opus", "high"), &in_week, 100.0), run("claude-b", ("opus", "high"), "2000-01-01T00:00:00Z", 900.0)];
    let window_runs = vec![run("claude-b", ("opus", "high"), &in_week, 10.0), run("claude-api", ("sonnet", "high"), &in_week, 7.0)];
    let account = AccountOutlook { account: "b".to_owned(), week_used: Some(50.0), week_resets: Some(resets), burn_per_hour: None, runway: Runway::Lasts };
    let screen = Screen { window: "24h".to_owned(), now: 0, bars: Err(String::new()), accounts: Ok(vec![account]), runs: Ok((window_runs, crate::ledger::ClosedReport::default())), attempts: Err(String::new()), week_runs: Ok(week_runs), billing: Err(String::new()) };
    let spent = super::spend::Spend::of(&screen).unwrap();
    assert_eq!(spent.classes.len(), 1);
    assert!((spent.classes[0].week_pct["b"] - 5.0).abs() < 1e-9, "{:?}", spent.classes[0]);
    assert_eq!((spent.api.runs, spent.api.usd), (1, 7.0));
}
