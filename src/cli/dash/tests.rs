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

fn infra(billing: &billing::Billing) -> Result<(Vec<crate::cli::infra::report::Resource>, Vec<String>), String> {
    Ok((crate::cli::infra::report::merge(Vec::new(), billing), Vec::new()))
}

fn work() -> Result<super::board::Work, String> {
    Ok(super::board::Work::default())
}

#[test]
fn the_screen_names_each_plan_and_machine_within_fifty_lines_of_120_characters_and_colours_only_on_request() {
    let billing = billing::parse(MANIFEST);
    let screen = Screen {
        window: "24h".to_owned(),
        now: 1_793_990_000,
        since: 1_793_903_600,
        bars: Err("unread".to_owned()),
        accounts: Ok(["a", "b", "c", "d"].map(account).to_vec()),
        runs: Err("unread".to_owned()),
        attempts: Err("unread".to_owned()),
        launches: Vec::new(),
        habits: Err("unread".to_owned()),
        week_runs: Err("unread".to_owned()),
        handovers: Ok(Default::default()),
        billing: billing.clone(),
        infra: infra(billing.as_ref().unwrap()),
        credit: Vec::new(),
        work: work(),
    };
    let text = render::screen(&screen, false);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() <= 50, "{} lines", lines.len());
    assert!(!text.contains('\x1b'));
    assert!(render::screen(&screen, true).contains("\x1b[3"));
    let start = lines.iter().position(|line| *line == "Accounts").expect("an accounts panel");
    let end = lines[start..].iter().position(|line| line.is_empty()).map_or(lines.len(), |gap| start + gap);
    assert!(lines.iter().all(|line| line.chars().count() <= 120), "{text}");
    let section = lines[start..end].join("\n");
    assert!(section.contains("Ultra $216.67/month"), "{section}");
    let start = lines.iter().position(|line| *line == "Infra").expect("an infra panel");
    let end = lines[start..].iter().position(|line| line.is_empty()).map_or(lines.len(), |gap| start + gap);
    let section = lines[start..end].join("\n");
    assert!(section.contains("$171.71"), "{section}");
    assert!(section.contains("$219.71"), "{section}");
}

fn run(account: &str, mix: (&str, &str), started: &str, usd: f64) -> crate::ledger::WindowRun {
    crate::ledger::WindowRun { agent: "x".to_owned(), role: "worker".to_owned(), category: "tooling".to_owned(), tier: String::new(), model: mix.0.to_owned(), effort: mix.1.to_owned(), account: account.to_owned(), started: started.to_owned(), outcome: "done".to_owned(), usd }
}

fn feature_run(agent: &str, tier: &str, model: &str, effort: &str, started: &str, usd: f64) -> crate::ledger::WindowRun {
    crate::ledger::WindowRun { agent: agent.to_owned(), role: "worker".to_owned(), category: "feature".to_owned(), tier: tier.to_owned(), model: model.to_owned(), effort: effort.to_owned(), account: "claude-b".to_owned(), started: started.to_owned(), outcome: "done".to_owned(), usd }
}

fn escalation_run(agent: &str, mix: (&str, &str), outcome: &str, started: &str, usd: f64) -> crate::ledger::WindowRun {
    crate::ledger::WindowRun { agent: agent.to_owned(), role: "worker".to_owned(), category: "tooling".to_owned(), tier: String::new(), model: mix.0.to_owned(), effort: mix.1.to_owned(), account: "claude-b".to_owned(), started: started.to_owned(), outcome: outcome.to_owned(), usd }
}

fn attempt(agent: &str, succeeded: bool) -> super::super::scoreboard::workers::Attempt {
    super::super::scoreboard::workers::Attempt {
        agent: agent.to_owned(),
        session: String::new(),
        item: String::new(),
        category: "feature".to_owned(),
        started: 0,
        ended: 0,
        succeeded,
        repos: std::collections::BTreeSet::new(),
        cause: crate::threads::attribution::Cause::Worker,
        basis: crate::threads::attribution::Basis::Done,
        deferred: false,
    }
}

fn launch(name: &str, tier: &str, effort: &str, signal: Option<&str>, lift: Option<&str>, at: f64) -> north_dispatch::dispatch::LaunchRecord {
    north_dispatch::dispatch::LaunchRecord {
        at,
        item: "north#940".to_owned(),
        host: "gex45".to_owned(),
        account: "claude:c".to_owned(),
        name: name.to_owned(),
        cloud: false,
        launched: true,
        detail: String::new(),
        tier: Some(tier.to_owned()),
        category: Some("feature".to_owned()),
        box_text: Some("Box: Done when: the escalation lands".to_owned()),
        hand: false,
        effort_arm: None,
        signal: signal.map(str::to_owned),
        lift: lift.map(str::to_owned),
        effort: Some(effort.to_owned()),
        cell: None,
        cell_role: None,
        route: None,
    }
}

#[test]
fn the_workers_section_shows_feature_boxes_cost_per_first_try_landing_and_rework_within_48_h_by_model_and_effort() {
    let started = "2026-11-09T01:00:00Z";
    let runs = vec![
        feature_run("s1", "sonnet-high", "claude-sonnet-5-5", "high", started, 6.0),
        feature_run("s2", "sonnet-high", "claude-sonnet-5-5", "high", started, 4.0),
        feature_run("o1", "opus-medium", "claude-opus-5-5", "medium", started, 20.0),
        escalation_run("e1", ("opus", "high"), "done", started, 20.0),
        escalation_run("e2", ("opus", "medium"), "not done", started, 10.0),
    ];
    let mut sonnet = crate::ledger::closed::Cell::priced("sonnet-high", "feature", 2, 10.0);
    sonnet.reworked = 1;
    let cell = |name: &str, usd: f64| crate::ledger::closed::CellBox { item: "north#1".to_owned(), category: "feature".to_owned(), cell: name.to_owned(), usd, closed: true, reworked: false };
    let report = crate::ledger::ClosedReport {
        cells: vec![sonnet, crate::ledger::closed::Cell::priced("opus-medium", "feature", 1, 20.0)],
        cell_boxes: vec![cell("sonnet-high", 10.0), cell("opus-medium", 20.0)],
        closed: 3,
        ..crate::ledger::ClosedReport::default()
    };
    let screen = Screen {
        window: "24h".to_owned(),
        now: 0,
        since: -86_400,
        bars: Err(String::new()),
        accounts: Err(String::new()),
        runs: Ok((runs, report)),
        attempts: Ok(vec![attempt("s1", true), attempt("s2", false), attempt("o1", true)]),
        launches: vec![launch("e1", "worker", "high", Some("reasoning"), None, -60.0), launch("e2", "worker", "medium", None, Some("e1"), -60.0)],
        habits: Err(String::new()),
        week_runs: Err(String::new()),
        handovers: Ok(Default::default()),
        billing: Err(String::new()),
        infra: Err(String::new()),
        credit: Vec::new(),
        work: work(),
    };
    let mixes = super::dispatch::Mixes::of(&screen).unwrap();
    let sonnet = mixes.rows.iter().find(|row| row.mix == "claude-sonnet-5-5 high").expect("the sonnet feature row");
    assert_eq!((sonnet.first_tries_landed, sonnet.closed, sonnet.reworked), (1, 2, 1));
    assert_eq!(sonnet.usd_per_first_try_landed, Some(10.0));
    let text = render::screen(&screen, false);
    let escalation = text.lines().find(|line| line.contains("escalation worker at high effort")).expect("the escalated rung's row");
    assert!(escalation.contains("1 climbed") && escalation.contains("1 of 1") && escalation.contains("+100 pts"), "the lift check compares the higher rung's landing with the parallel lower one: {escalation}");
    assert!(escalation.chars().count() <= 120, "{escalation}");
}

#[test]
fn a_class_uses_the_share_of_a_week_its_dollars_are_of_the_week_sized_from_the_spend_so_far_and_api_credit_runs_stay_apart() {
    let resets = Timestamp::from_unix_seconds(1_794_000_000);
    let in_week = Timestamp::from_unix_seconds(1_794_000_000 - 86_400).iso();
    let week_runs = vec![run("claude-c", ("opus", "high"), &in_week, 40.0), run("claude-b", ("opus", "high"), &in_week, 100.0), run("claude-b", ("opus", "high"), "2000-01-01T00:00:00Z", 900.0)];
    let window_runs = vec![run("claude-b", ("opus", "high"), &in_week, 10.0), run("claude-api", ("sonnet", "high"), &in_week, 7.0)];
    let account = |letter: &str, used: f64| AccountOutlook { account: letter.to_owned(), week_used: Some(used), week_resets: Some(resets), burn_per_hour: None, runway: Runway::Lasts };
    let screen = Screen { window: "24h".to_owned(), now: 0, since: -86_400, bars: Err(String::new()), accounts: Ok(vec![account("b", 50.0), account("c", 99.0)]), runs: Ok((window_runs, crate::ledger::ClosedReport::default())), attempts: Err(String::new()), launches: Vec::new(), habits: Err(String::new()), week_runs: Ok(week_runs), handovers: Ok(Default::default()), billing: Err(String::new()), infra: Err(String::new()), credit: Vec::new(), work: work() };
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
    record("ambient-test-input", "rewrite", "2026-11-09T13:00:00Z");

    let rows = super::habits::rows(&path, "2026-11-09T00:00:00Z", "2026-11-09T12:00:00Z").unwrap();
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
        since: 1_793_903_600,
        bars: Err(String::new()),
        accounts: Err(String::new()),
        runs: Err(String::new()),
        attempts: Err(String::new()),
        launches: Vec::new(),
        habits: Ok(rows),
        week_runs: Err(String::new()),
        handovers: Ok(Default::default()),
        billing: Err(String::new()),
        infra: Err(String::new()),
        credit: Vec::new(),
        work: work(),
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
    let screen = Screen { window: "24h".to_owned(), now: 1_793_990_000, since: 1_793_903_600, bars: Ok((0..15).map(|_| bar()).collect()), accounts: Ok(accounts), runs: Ok((runs, crate::ledger::ClosedReport::default())), attempts: Ok(Vec::new()), launches: Vec::new(), habits: Ok(habits), week_runs: Ok(Vec::new()), handovers: Ok(Default::default()), billing: billing::parse(MANIFEST), infra: Err(String::new()), credit: Vec::new(), work: work() };
    let text = render::screen(&screen, false);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() <= 50, "{text}");
    assert!(lines.iter().all(|line| line.chars().count() <= 120), "{text}");
    for title in ["Work", "Accounts", "Infra", "Health"] {
        assert!(lines.contains(&title), "{title} missing:\n{text}");
    }
    assert!(lines.iter().any(|line| line.contains("all runs")), "the accounts table has no row:\n{text}");
}
