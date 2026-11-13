use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::{Sources, rows};
use crate::fleet::landings::Push;
use crate::ledger::Run;
use crate::providers::claude::runs::landing::MainIndex;
use crate::providers::claude::runs::telemetry::Prices;
use crate::providers::claude::runs::{CATALOG, DEFAULTS};
use crate::providers::ledger_rows::Shared;

const AT: i64 = 1_794_300_000_000;

fn turn(session: &str, started_ms: i64) -> Run {
    Run {
        run_id: started_ms,
        session: session.into(),
        task_ref: Some("north#1020".into()),
        provider: "opencode".into(),
        model: Some("opencode-go/deepseek-v4.1-flash".into()),
        effort: Some("high".into()),
        overlay: None,
        account: Some("go".into()),
        prompt_sha: None,
        system_prompt_sha: None,
        deepen: false,
        started_ms,
        ended_ms: started_ms + 60_000,
        wall_ms: 60_000,
        turns: 3,
        tokens_in: 1000,
        tokens_out: 200,
        tokens_cache_read: 500,
        tokens_cache_write: 0,
        tool_calls: Some(4),
        compactions: Some(0),
        cost_usd: Some(0.25),
        exit: "completed".into(),
    }
}

/// On 2026-11-10 `north ledger opencode` found no DeepSeek run although dozens of opencode sessions had run and landed:
/// the worker ledger read only Claude, codex and agy stores, and a DeepSeek commit lands under its Sonnet reviewer's name.
#[test]
fn a_finished_deepseek_session_is_a_worker_run_that_its_reviewers_landing_joins() {
    let (known, recorded) = (HashSet::new(), HashSet::new());
    let (main, prices) = (MainIndex::parse(""), Prices::parse(&CATALOG, &DEFAULTS));
    let shared = Shared { session: "", since: "", known: &known, recorded: &recorded, main: &main, prices: &prices };
    let turns = [turn("ds-done", AT), turn("ds-done", AT + 120_000), turn("ds-busy", AT)];
    let prompts = HashMap::from([("ds-done".to_owned(), "Item: north#1020 a box\nCategory: feature\nDo it.".to_owned())]);
    let nodes = [json!({"name": "ds-busy", "status": "working"}), json!({"name": "ds-done", "status": "idle"})];
    let push = |session: &str, oid: &str, landed: &str| Push { at_ms: (AT + 600_000) as u64, repository: "north".into(), branch: "main".into(), oid: oid.into(), landed: landed.into(), session: session.into() };
    let pushes = [push("ds-done-rv", "abc", "def"), push("other", "zzz", "yyy"), push("", "lane-head", "landed-head")];
    let reviewed = HashMap::from([("ds-done".to_owned(), vec!["lane-head".to_owned()])]);
    let sources = Sources { turns: &turns, prompts: &prompts, nodes: Some(&nodes), pushes: &pushes, reviewed: &reviewed };
    let out = rows(&shared, &sources, (AT / 1000) as f64);
    assert_eq!(out.len(), 3, "{out:?}");
    let run = &out[0];
    assert_eq!((run["id"].as_str(), run["fleet_name"].as_str(), run["provider"].as_str(), run["role"].as_str()), (Some("ds-done"), Some("ds-done"), Some("opencode"), Some("worker")));
    assert!(run["brief"].as_str().is_some_and(|brief| brief.contains("Category: feature")), "{run}");
    assert_eq!((run["brief_text"].as_str(), run["active_min"].as_f64(), run["wait_min"].as_f64()), (prompts.get("ds-done").map(String::as_str), Some(2.0), Some(1.0)), "on 2026-11-13 every opencode and pi run of the day lacked brief text and active minutes: {run}");
    assert_eq!((run["tool_calls"].as_i64(), run["compactions"].as_i64(), run["cost_usd"].as_f64()), (Some(8), Some(0), Some(0.5)), "on 2026-11-14 a third of the day's runs, the opencode and pi ones, lacked tool calls, compactions and cost: {run}");
    assert_eq!(run["ended"].as_str(), Some(crate::foundation::clock::Timestamp::from_unix_millis(AT + 180_000).iso().as_str()));
    let landed: Vec<&str> = out[1..].iter().filter(|row| row["push_join"] == 1 && row["fleet_name"] == "ds-done").filter_map(|row| row["landed_sha"].as_str()).collect();
    assert_eq!(landed, ["def", "landed-head"]);
}

/// On 2026-11-10 the ledger's newest recorded run lagged finished DeepSeek workers by up to 42 min: a session host's
/// end records its run by conversation id, and an opencode conversation id never equals the fleet name its turns carry.
#[test]
fn a_session_end_naming_an_opencode_conversation_records_that_sessions_run() {
    let (known, recorded) = (HashSet::new(), HashSet::new());
    let (main, prices) = (MainIndex::parse(""), Prices::parse(&CATALOG, &DEFAULTS));
    let shared = Shared { session: "ses_2e3ba1aa88d8", since: "", known: &known, recorded: &recorded, main: &main, prices: &prices };
    let turns = [turn("ds-done", AT), turn("ds-other", AT)];
    let nodes = [json!({"name": "ds-done", "conversation": "ses_2e3ba1aa88d8", "status": "idle"}), json!({"name": "ds-other", "conversation": "ses_ffff", "status": "idle"})];
    let (prompts, reviewed) = (HashMap::new(), HashMap::new());
    let sources = Sources { turns: &turns, prompts: &prompts, nodes: Some(&nodes), pushes: &[], reviewed: &reviewed };
    let out = rows(&shared, &sources, (AT / 1000) as f64);
    let ids: Vec<&str> = out.iter().filter_map(|row| row["id"].as_str()).collect();
    assert_eq!(ids, ["ds-done"]);
}
