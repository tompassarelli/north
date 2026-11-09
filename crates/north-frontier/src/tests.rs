use crate::report::render;
use crate::rows::{Arm, Run, parse};
use crate::stats::{Pairs, mcnemar, wilson};

fn run(task: &str, arm: Arm, first: bool) -> Run {
    Run { task: task.into(), arm, model: "claude-sonnet-5-5".into(), effort: "high".into(), correct_first_pass: first, correct: first, cost_usd: 2.0, wall_seconds: 600.0, interventions: 0, contaminated: false }
}

#[test]
fn wilson_matches_the_published_interval_for_eight_of_ten() {
    let interval = wilson(8, 10).unwrap();
    assert!((interval.low - 0.4902).abs() < 1e-3 && (interval.high - 0.9433).abs() < 1e-3);
}

#[test]
fn mcnemar_is_exact_on_the_discordant_split() {
    assert!((mcnemar(Pairs { both: 5, only_first: 6, only_second: 1, neither: 0 }) - 0.125).abs() < 1e-12);
    assert_eq!(mcnemar(Pairs { both: 3, only_first: 0, only_second: 0, neither: 2 }), 1.0);
}

#[test]
fn a_contaminated_pair_leaves_the_headline() {
    let mut tainted = run("a", Arm::Plain, true);
    tainted.contaminated = true;
    let text = render(&[run("a", Arm::North, true), tainted, run("b", Arm::North, true), run("b", Arm::Plain, false)]);
    assert!(text.contains("on 1 boxes both arms ran (1 left out"), "{text}");
    assert!(text.contains("north minus plain: +100 points"), "{text}");
}

#[test]
fn rows_parse_from_json_lines_and_name_the_bad_line() {
    let good = r#"{"task":"north#1","arm":"north","model":"claude-sonnet-5-5","effort":"high","correct_first_pass":true,"correct":true,"cost_usd":1.5,"wall_seconds":90.0,"interventions":1}"#;
    assert_eq!(parse(&format!("{good}\n\n{good}\n")).unwrap().len(), 2);
    assert!(parse(&format!("{good}\nnot json\n")).unwrap_err().contains(":2:"));
}

#[test]
fn the_task_id_in_a_printed_path_is_not_a_leak() {
    let landed = "4082956297aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let path = "safe-push: /pilot/north-685-4082956297/plain-1/bin/safe-push landed 9c58a7d2";
    assert!(crate::grade::reached(path, landed).is_empty());
    assert_eq!(crate::grade::reached("git show 4082956297aaa", landed), ["the landed sha"]);
}

// KEEP(bug): both arms of north-636 sat idle for twenty minutes each after the API refused their first turn for credit, and the pass spent its time on runs with no work in them (north#730)
#[test]
fn a_credit_refusal_ends_the_arm_and_a_finished_turn_does_not() {
    let refusal = serde_json::json!({"type": "result", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"});
    let done = serde_json::json!({"type": "result", "is_error": false, "result": "Done: landed"});
    assert!(crate::session::refused_for_credit(&refusal));
    assert!(!crate::session::refused_for_credit(&done));
}

// KEEP(bug): frontier runs drew pool keys on 2026-11-07 and wrote no ledger run, so the pool counted none of that spend against the org whose key served them (north#792)
#[test]
fn an_arm_that_ran_on_a_pool_key_is_charged_to_that_orgs_account_in_the_ledger() {
    let results = [
        serde_json::json!({"type": "result", "modelUsage": {"claude-sonnet-5-5": {"inputTokens": 10, "outputTokens": 20, "cacheReadInputTokens": 300, "cacheCreationInputTokens": 40}}}),
        serde_json::json!({"type": "result", "modelUsage": {"claude-sonnet-5-5": {"inputTokens": 15, "outputTokens": 25, "cacheReadInputTokens": 350, "cacheCreationInputTokens": 45}, "claude-haiku-5-5": {"inputTokens": 1, "outputTokens": 2}}}),
    ];
    let spent = crate::billing::Spent::of("frontier-a-north-1", &results, 1_000, 5_000);
    let dir = std::env::temp_dir().join(format!("north-frontier-charge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ledger.db");
    crate::billing::charge(&north_providers::ledger::Ledger::open(&path).unwrap(), "proton", &spent).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let mut rows: Vec<(String, String, i64, i64, i64, i64)> = db
        .prepare("SELECT account, model, tokens_in, tokens_out, tokens_cache_read, tokens_cache_write FROM runs ORDER BY model")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    rows.sort();
    assert_eq!(rows, [("api:proton".into(), "claude-haiku-5-5".into(), 1, 2, 0, 0), ("api:proton".into(), "claude-sonnet-5-5".into(), 15, 25, 350, 45)]);
}

// KEEP(bug): north-633's north arm landed its change and reported `Not done:` with `Stop: host laptop` because the live check needs the laptop, and the runner answered it as a question, charging north an intervention and an idle wait no continuation could shorten (north#855)
#[test]
fn a_stop_on_another_machine_is_not_a_question_and_a_decision_still_is() {
    let laptop = "Not done: the trace recorder is landed as 907ad9aa on origin/main, but nothing has produced a real trace yet.\n\nStop: host laptop";
    let client = "Blocked: the fix is landed; the replay needs a native client.\nStop: client";
    let decision = "Not done: two designs fit the brief.\nStop: decision";
    assert!(!crate::session::asks(laptop) && !crate::session::asks(client));
    assert!(crate::session::asks(decision) && crate::session::asks("Done: landed. Should I also port the bash twin?"));
}

fn ab_row(arm: Arm, index: i64, minutes: i64, usd: f64) -> crate::ab::AbRow {
    crate::ab::AbRow {
        task: format!("wisp#{index}"),
        arm,
        model: "claude-sonnet-5-5".into(),
        effort: "high".into(),
        assigned_at: 1_000_000 + index * 10_000,
        landed_sha: Some(format!("{index:040x}")),
        landed_at: Some(1_000_000 + index * 10_000 + minutes * 60),
        usage: crate::ab::Usage { tokens_in: 1_000, tokens_out: 4_000, cost_usd: usd },
    }
}

#[test]
fn the_live_ab_waits_for_thirty_landed_per_arm_then_reports_time_and_short_and_long_term_usage_in_the_doc() {
    let mut rows: Vec<_> = (0..29).flat_map(|index| [ab_row(Arm::North, index, 20 + index % 5, 1.0 + (index % 3) as f64), ab_row(Arm::T3Code, 100 + index, 60 + index % 7, 10.0 + (index % 3) as f64)]).collect();
    assert!(!crate::ab::ready(&rows) && crate::ab::progress(&rows).contains("north 29/30, t3code 29/30"));
    rows.push(ab_row(Arm::North, 29, 22, 1.0));
    rows.push(ab_row(Arm::T3Code, 129, 61, 10.0));
    assert!(crate::ab::ready(&rows));
    let long_term: crate::ab::LongTerm = rows.iter().map(|row| (row.task.clone(), crate::ab::Usage { tokens_in: 100, tokens_out: 900, cost_usd: if row.arm == Arm::North { 0.5 } else { 2.5 } })).collect();
    let section = crate::ab::render(&rows, &long_term);
    for line in ["assignment to landed, time, north: median", "per landed box, API-equivalent cost, t3code: median $11", "per landed box, subscription usage (tokens in and out), north: median 5k tokens, total 150k tokens", "per box, API-equivalent cost, north: median $0.50", "north minus t3code: median -9.00 USD", "30 of 30 north boxes and 30 of 30 t3code boxes are past 14 days"] {
        assert!(section.contains(line), "{line}\n{section}");
    }
    assert!(!section.contains("verdict") && !section.contains("rework"), "{section}");
    let doc = crate::ab::splice("# Frontier\n\ntext\n", &section);
    assert!(doc.contains("## Live A/B result") && doc.starts_with("# Frontier"));
    assert_eq!(crate::ab::splice(&doc, &section), doc);
}

// KEEP(bug): the long-term usage must follow the box's own lines by blame: a later commit that edits other lines of the same file, or edits the box's lines after its 14 days, is not rework on the box (north#877)
#[test]
fn long_term_usage_counts_only_commits_inside_14_days_that_change_the_boxs_lines() {
    let dir = std::env::temp_dir().join(format!("north-frontier-follow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let epoch = 1_000_000_000;
    let git = |at: i64, arguments: &[&str]| {
        let date = format!("{} +0000", at + epoch);
        let output = std::process::Command::new("git").arg("-C").arg(&dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).env("GIT_COMMITTER_DATE", &date).env("GIT_AUTHOR_DATE", &date).output().unwrap();
        assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let day = 86_400;
    let commit = |at: i64, text: &str| {
        std::fs::write(dir.join("file.txt"), text).unwrap();
        git(at, &["commit", "-qam", "change"]);
        git(at, &["rev-parse", "HEAD"])
    };
    git(0, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("file.txt"), "base\n").unwrap();
    git(0, &["add", "file.txt"]);
    git(0, &["commit", "-qm", "base"]);
    let landed = commit(day, "base\nbox one\nbox two\n");
    let elsewhere = commit(2 * day, "head\nbase\nbox one\nbox two\n");
    let fix = commit(3 * day, "head\nbase\nbox ONE\nbox two\n");
    let late = commit(20 * day, "head\nbase\nbox ONE\nbox TWO\n");
    assert_eq!(crate::followup::changing_commits(&dir, "HEAD", &landed, epoch + day).unwrap(), std::slice::from_ref(&fix));
    let spend = crate::followup::Spend::of([(fix, crate::ab::Usage { tokens_in: 7, tokens_out: 3, cost_usd: 1.5 }), (elsewhere, crate::ab::Usage { tokens_in: 99, tokens_out: 99, cost_usd: 9.0 }), (late, crate::ab::Usage { tokens_in: 99, tokens_out: 99, cost_usd: 9.0 })].into());
    let mut row = ab_row(Arm::North, 1, 1, 0.0);
    (row.landed_sha, row.landed_at) = (Some(landed), Some(epoch + day));
    let found = crate::followup::long_term(&dir, "HEAD", &spend, &[row.clone()], epoch + 15 * day);
    assert_eq!(found.get("wisp#1"), Some(&crate::ab::Usage { tokens_in: 7, tokens_out: 3, cost_usd: 1.5 }));
    assert!(crate::followup::long_term(&dir, "HEAD", &spend, &[row], epoch + 15 * day - 1).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_live_ab_row_for_the_plain_arm_is_refused() {
    let row = r#"{"box":"wisp#1","arm":"plain","model":"m","effort":"e","assigned_at":1}"#;
    assert!(crate::ab::parse(row).unwrap_err().contains("not plain"));
}

#[test]
fn an_assigned_box_is_an_ab_row_the_report_parses_and_the_seeded_coin_never_changes_its_arm() {
    let dir = std::env::temp_dir().join(format!("north-frontier-ab-{}", std::process::id()));
    let file = dir.join("ab.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    let arm = crate::assign::flip(877, "wisp#12:ship-the-thing");
    assert_eq!(arm, crate::assign::flip(877, "wisp#12:ship-the-thing"));
    let row = crate::ab::AbRow { task: "wisp#12:ship-the-thing".into(), arm, assigned_at: 1_794_000_000, model: "m".into(), effort: "e".into(), landed_sha: None, landed_at: None, usage: crate::ab::Usage::default() };
    std::fs::write(&file, serde_json::to_string(&row).unwrap() + "\n").unwrap();
    let landing = crate::assign::Landing { task: "wisp#12:ship-the-thing", sha: "abc1234", at: 1_794_000_600, usage: crate::ab::Usage { tokens_in: 1, tokens_out: 2, cost_usd: 0.5 }, model: Some("claude-sonnet-5-5".into()), effort: Some("high".into()) };
    crate::assign::record(&file, &landing).unwrap();
    crate::assign::record(&file, &landing).unwrap();
    let rows = crate::ab::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!((rows[0].landed_sha.as_deref(), rows[0].landed_at, rows[0].usage.cost_usd), (Some("abc1234"), Some(1_794_000_600), 1.0));
}

#[test]
fn a_box_thread_runs_on_t3_codes_own_default_with_the_box_text_as_its_brief() {
    let base = std::env::temp_dir().join(format!("north-live-settings-{}", std::process::id()));
    crate::t3code::write_settings(&base, crate::live::MODEL, crate::live::EFFORT).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(base.join("userdata/settings.json")).unwrap()).unwrap();
    assert_eq!(saved["defaultModelSelection"], serde_json::json!({"instanceId": "claudeAgent", "model": "claude-opus-5-5", "options": [{"id": "effort", "value": "medium"}]}));
    assert_eq!(crate::live::brief("  the box text\n"), "the box text");
    let picked = crate::assign::Box { id: "wisp#1:x".into(), text: "x".into(), rank: 0, number: 1 };
    let t3 = crate::assign::picked_row(&picked, Arm::T3Code, 1);
    assert_eq!((t3.model.as_str(), t3.effort.as_str()), (crate::live::MODEL, crate::live::EFFORT));
    std::fs::remove_dir_all(&base).unwrap();
    for flag in ["--model", "--effort", "--header"] {
        let arguments: Vec<String> = ["box", "wisp#1", flag, "x"].map(str::to_owned).to_vec();
        assert!(crate::cli::dispatch(&arguments).unwrap_err().to_string().contains("T3 Code's own default"));
    }
}
