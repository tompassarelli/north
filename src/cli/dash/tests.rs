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
        habits: Err("unread".to_owned()),
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
    let week_runs = vec![run("claude-c", ("opus", "high"), &in_week, 40.0), run("claude-b", ("opus", "high"), &in_week, 100.0), run("claude-b", ("opus", "high"), "2000-01-01T00:00:00Z", 900.0)];
    let window_runs = vec![run("claude-b", ("opus", "high"), &in_week, 10.0), run("claude-api", ("sonnet", "high"), &in_week, 7.0)];
    let account = |letter: &str, used: f64| AccountOutlook { account: letter.to_owned(), week_used: Some(used), week_resets: Some(resets), burn_per_hour: None, runway: Runway::Lasts };
    let screen = Screen { window: "24h".to_owned(), now: 0, bars: Err(String::new()), accounts: Ok(vec![account("b", 50.0), account("c", 99.0)]), runs: Ok((window_runs, crate::ledger::ClosedReport::default())), attempts: Err(String::new()), habits: Err(String::new()), week_runs: Ok(week_runs), billing: Err(String::new()) };
    let spent = super::spend::Spend::of(&screen).unwrap();
    assert_eq!(spent.classes.len(), 1);
    assert!((spent.classes[0].week_pct["b"] - 5.0).abs() < 1e-9, "{:?}", spent.classes[0]);
    assert_eq!(spent.classes[0].week_pct["c"], 0.0, "a sized week the class did not touch reads 0, not absent");
    assert_eq!((spent.api.runs, spent.api.usd), (1, 7.0));
}

#[test]
fn the_habit_section_counts_each_rules_rewrites_and_warnings_inside_the_window() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    crate::threads::runs::hooks::ensure(&db).unwrap();
    let record = |rule: &str, verdict: &str, at: &str| {
        db.execute(
            "INSERT INTO hook_events(session, tool_use, tool, event, verdict, rule, at, turn) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params!["s", format!("{rule}-{verdict}-{at}"), "Edit", "PostToolUse", verdict, rule, at, 1],
        )
        .unwrap();
    };
    record("ambient-test-input", "rewrite", "2026-11-09T01:00:00Z");
    record("ambient-test-input", "advisory", "2026-11-09T02:00:00Z");
    record("shared-mutability", "rewrite", "2026-11-09T03:00:00Z");
    record("other", "advisory", "2026-11-09T03:00:00Z");
    record("ambient-test-input", "rewrite", "2026-11-01T03:00:00Z");

    let rows = super::habits::rows(&path, "2026-11-09T00:00:00Z").unwrap();
    assert_eq!(
        rows,
        [
            super::habits::Row { rule: "ambient-test-input".to_owned(), rewrites: 1, warnings: 1 },
            super::habits::Row { rule: "shared-mutability".to_owned(), rewrites: 1, warnings: 0 },
        ]
    );
    let screen = Screen {
        window: "24h".to_owned(),
        now: 1_793_990_000,
        bars: Err(String::new()),
        accounts: Err(String::new()),
        runs: Err(String::new()),
        attempts: Err(String::new()),
        habits: Ok(rows),
        week_runs: Err(String::new()),
        billing: Err(String::new()),
    };
    let text = render::screen(&screen, false);
    assert!(text.contains("2 rewrites and 1 warning on 2 rules"), "{text}");
    assert!(text.contains("ambient-test-input"), "{text}");
    assert!(text.lines().count() <= 50, "{text}");
}

#[test]
fn the_three_sections_keep_a_worker_row_when_every_bar_fails_and_the_habit_table_is_full() {
    use super::ready;
    let bar = || ready::Line { repo: "north", bar: "bar", plain: "bar", target: ">= 99%".to_owned(), verdict: ready::Verdict::Missed, hour: None, hour_n: None, window: None, window_n: None, five_text: "99% (n=1)".to_owned(), fifteen_text: "99% (n=1)".to_owned(), hour_text: "99% (n=1)".to_owned(), window_text: "99% (n=1)".to_owned() };
    let accounts: Vec<_> = ["a", "b", "c", "d", "e", "f", "g", "h"].iter().map(|name| account(name)).collect();
    let runs: Vec<_> = (0..20).map(|index| run(&format!("claude-{}", (b'a' + (index % 8) as u8) as char), ("opus", "high"), "2026-11-09T00:00:00Z", 1.0)).collect();
    let habits = vec![
        super::habits::Row { rule: "ambient-test-input".to_owned(), rewrites: 2, warnings: 1 },
        super::habits::Row { rule: "shared-mutability".to_owned(), rewrites: 1, warnings: 0 },
        super::habits::Row { rule: "single-impl-trait".to_owned(), rewrites: 0, warnings: 3 },
    ];
    let screen = Screen { window: "24h".to_owned(), now: 1_793_990_000, bars: Ok((0..15).map(|_| bar()).collect()), accounts: Ok(accounts), runs: Ok((runs, crate::ledger::ClosedReport::default())), attempts: Ok(Vec::new()), habits: Ok(habits), week_runs: Ok(Vec::new()), billing: billing::parse(MANIFEST) };
    let text = render::screen(&screen, false);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() <= 50, "{text}");
    assert!(lines.iter().all(|line| line.chars().count() <= 120), "{text}");
    for title in ["Key metrics", "Billing:", "Workers:"] {
        assert!(lines.iter().any(|line| line.starts_with(title)), "{title} missing:\n{text}");
    }
    assert!(lines.iter().any(|line| line.starts_with("all categories")), "the worker table has no row:\n{text}");
}
