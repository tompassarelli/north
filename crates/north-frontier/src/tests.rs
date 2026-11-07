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
