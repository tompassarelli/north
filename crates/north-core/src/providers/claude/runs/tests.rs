use std::fs;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::telemetry::{Prices, run_usage};
use super::landing::{MainIndex, checks, settle};
use crate::fleet::landings::Push;
use super::{CATALOG, DEFAULTS, roots};

const PRICED: &str = r#"
[models."claude:opus-medium"]
model = "claude-opus-5-5"
[models."claude:opus-medium".value.api]
input_per_million = 4.0
output_per_million = 20.0
[models."claude:haiku"]
model = "claude-haiku-5-5"
[models."claude:haiku".value.api]
input_per_million = 0.1
output_per_million = 0.5
"#;

fn turn(model: &str, request: &str, usage: Value) -> Value {
    json!({"type": "assistant", "timestamp": "2026-10-11T00:05:00Z", "requestId": request, "message": {"model": model, "usage": usage}})
}

fn weight(prices: &Prices, model: &str) -> (f64, Value) {
    let usage = json!({"input_tokens": 1000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 1000, "output_tokens": 100});
    let telemetry = run_usage(&[turn(model, model, usage)], "claude-b", prices);
    (telemetry["usage_samples"][0]["weight"].as_f64().unwrap(), telemetry["unpriced_models"].clone())
}

#[test]
fn a_streamed_request_counts_once_at_token_weight_when_its_model_has_no_list_price() {
    let usage = json!({"input_tokens": 100, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 100, "output_tokens": 100, "speed": "fast"});
    let record = turn("claude-opus-4-6", "r", usage);
    let telemetry = run_usage(&[record.clone(), record], "claude-b", &Prices::parse("", DEFAULTS));
    assert_eq!(telemetry["fast"], 1);
    assert_eq!(telemetry["account"], "claude-b");
    assert_eq!(telemetry["usage_samples"].as_array().unwrap().len(), 1);
    assert_eq!(telemetry["usage_samples"][0]["weight"], 730.0);
    assert_eq!(telemetry["unpriced_models"], json!(["claude-opus-4-6"]));
}

#[test]
fn list_prices_weight_opus_forty_times_haiku_and_an_uncatalogued_model_takes_token_weight() {
    let prices = Prices::parse(PRICED, DEFAULTS);
    let (opus, opus_unpriced) = weight(&prices, "claude-opus-5-5");
    let (haiku, _) = weight(&prices, "claude-haiku-5-5");
    let (sonnet, sonnet_unpriced) = weight(&prices, "claude-sonnet-9");
    assert_eq!((opus, haiku, sonnet), (6400.0, 160.0, 1600.0));
    assert!((opus / haiku - 40.0).abs() < 1e-9);
    assert_eq!(opus_unpriced, json!([]));
    assert_eq!(sonnet_unpriced, json!(["claude-sonnet-9"]));
}

#[test]
fn the_built_in_catalog_prices_every_claude_model_it_names() {
    let prices = Prices::parse(CATALOG, DEFAULTS);
    for model in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] {
        assert_eq!(weight(&prices, model).1, json!([]), "{model}");
    }
}

#[test]
fn a_root_declared_at_claudes_default_home_takes_its_label_from_the_config_beside_it() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude/projects")).unwrap();
    fs::create_dir_all(home.path().join("profile/projects")).unwrap();
    fs::write(home.path().join(".claude.json"), json!({"oauthAccount": {"accountUuid": "account-one"}}).to_string()).unwrap();
    let identity = &format!("{:x}", Sha256::digest(b"account-one"))[..8];
    let policy = format!("[accounts.claude]\nlabels = {{ \"{identity}\" = \"a\" }}\nprofiles = {{ a = {{ here = {{ path = \"~/.claude\" }} }} }}\n");

    let roots = roots(home.path(), Some(home.path().join("profile")), Some(&policy.parse().unwrap()), "here");

    let accounts: Vec<&str> = roots.iter().map(|root| root.account.as_str()).collect();
    assert_eq!(accounts, ["claude", "claude-a"]);
}

#[test]
fn a_token_profile_root_without_a_login_takes_the_label_it_is_declared_under_even_as_the_running_config_dir() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("token-home/projects")).unwrap();
    let policy = "[accounts.claude]\nprofiles = { b = { here = { path = \"~/token-home\", token_file = \"~/token-b\" } } }\n";

    let roots = roots(home.path(), Some(home.path().join("token-home")), Some(&policy.parse().unwrap()), "here");

    let accounts: Vec<&str> = roots.iter().map(|root| root.account.as_str()).collect();
    assert_eq!(accounts, ["claude-b"]);
}

#[test]
fn a_landed_commit_subject_naming_an_issue_is_reported_as_the_runs_landed_ref() {
    let sha = "a".repeat(40);
    let records = [
        json!({"type": "assistant", "timestamp": "2026-10-28T09:00:00Z", "message": {"content": [{"type": "tool_use", "id": "c", "name": "Bash", "input": {"command": "git commit -m \"Fix the thing (north#42)\""}}]}}),
        json!({"type": "user", "timestamp": "2026-10-28T09:01:00Z", "message": {"content": [{"type": "tool_result", "tool_use_id": "c", "content": "[main aaaaaaa] Fix the thing (north#42)"}]}}),
    ];
    let main = MainIndex::parse(&format!("{sha} 2026-10-28T10:00:00Z Fix the thing (north#42)\n"));
    let landed = checks(&records, &main);
    assert_eq!((landed["landed"].clone(), landed["landed_ref"].clone()), (json!(1), json!("north#42")));
    let elsewhere = checks(&records, &MainIndex::parse(&format!("{} 2026-10-28T10:00:00Z Another change (north#7)\n", "b".repeat(40))));
    assert_eq!((elsewhere["landed"].clone(), elsewhere["landed_ref"].clone()), (json!(0), Value::Null));
}

#[test]
fn a_run_whose_session_lists_another_lanes_earlier_landing_records_its_own_landing() {
    let (own, other) = ("7".repeat(40), "1".repeat(40));
    let step = |id: &str, at: &str, command: &str, output: &str| {
        [
            json!({"type": "assistant", "timestamp": at, "message": {"content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}}]}}),
            json!({"type": "user", "timestamp": at, "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": output}]}}),
        ]
    };
    let records: Vec<Value> = [
        step("c", "2026-10-29T11:12:12Z", "git commit -q -m \"Reread a settled run (north#255)\"", ""),
        step("l", "2026-10-29T11:20:00Z", "git log --oneline origin/main -3; git diff --stat origin/main..n112-tune", "1111111 Pin the tune route cell (north#112)\n0000000 Older change (north#7)"),
        step("p", "2026-10-29T11:45:20Z", "safe-push", &format!("safe-push: landed {own}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let main = MainIndex::parse(&format!("{other} 2026-10-29T11:15:43Z Pin the tune route cell (north#112)\n{own} 2026-10-29T11:45:10Z Reread a settled run (north#255)\n"));

    let landed = checks(&records, &main);

    assert_eq!((landed["landed_sha"].as_str(), landed["landed_ref"].as_str()), (Some(own.as_str()), Some("north#255")));
}

#[test]
fn a_run_whose_landing_reports_only_in_a_background_notice_records_that_landing_over_another_lanes_listed_commit() {
    let (own, other) = ("f".repeat(40), "2".repeat(40));
    let landed = format!("land check: bin/threads.test.sh passed after 254 s\nsafe-push: landed {own}\nexit 0");
    let notice = format!("<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<event>{landed}</event>\n</task-notification>");
    let call = |id: &str, name: &str, input: Value| json!({"type": "assistant", "timestamp": "2026-10-29T15:40:00Z", "message": {"content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}});
    let result = |id: &str, output: &str| json!({"type": "user", "timestamp": "2026-10-29T15:40:01Z", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": output}]}});
    let deliveries = [
        vec![json!({"type": "user", "timestamp": "2026-10-29T16:07:37Z", "origin": {"kind": "task-notification"}, "message": {"role": "user", "content": notice}})],
        vec![json!({"type": "attachment", "timestamp": "2026-10-29T16:07:37Z", "attachment": {"type": "queued_command", "commandMode": "task-notification", "prompt": notice}})],
        vec![call("r", "Read", json!({"file_path": "/tmp/safepush.log"})), result("r", &landed.lines().enumerate().map(|(n, line)| format!("{:>6}\t{line}\n", n + 1)).collect::<String>())],
    ];
    let main = MainIndex::parse(&format!("{other} 2026-10-29T15:16:42Z Abandon a stale runtime build (north#308)\n{own} 2026-10-29T15:23:22Z Read the fleet registry in-process (north#309)\n"));
    for delivery in deliveries {
        let records: Vec<Value> = [
            call("c", "Bash", json!({"command": "git commit -q --amend --no-edit"})),
            result("c", ""),
            call("b", "Bash", json!({"command": "git bundle create /tmp/lane.bundle origin/main..HEAD; ssh gex45 'cd ~/code/north/main && git log --oneline -1'"})),
            result("b", "2222222 Abandon a stale runtime build (north#308)"),
            call("p", "Bash", json!({"command": "(safe-push --to main > /tmp/safepush.log 2>&1; echo exit $? >> /tmp/safepush.log) &", "run_in_background": true})),
            result("p", "bg"),
        ]
        .into_iter()
        .chain(delivery.clone())
        .collect();

        let landing = checks(&records, &main);

        assert_eq!((landing["landed_sha"].as_str(), landing["landed_ref"].as_str()), (Some(own.as_str()), Some("north#309")), "{delivery:?}");
    }
}

#[test]
fn a_run_known_only_by_its_lane_listing_lands_the_listed_commit() {
    let sha = "a".repeat(40);
    let records = [
        json!({"type": "assistant", "timestamp": "2026-10-28T09:00:00Z", "message": {"content": [{"type": "tool_use", "id": "c", "name": "Bash", "input": {"command": "git commit --amend --no-edit"}}, {"type": "tool_use", "id": "l", "name": "Bash", "input": {"command": "git log --oneline origin/main..HEAD"}}]}}),
        json!({"type": "user", "timestamp": "2026-10-28T09:01:00Z", "message": {"content": [{"type": "tool_result", "tool_use_id": "c", "content": ""}, {"type": "tool_result", "tool_use_id": "l", "content": "aaaaaaa Fix the thing (north#42)"}]}}),
    ];
    let main = MainIndex::parse(&format!("{sha} 2026-10-28T10:00:00Z Fix the thing (north#42)\n"));

    assert_eq!(checks(&records, &main)["landed_sha"].as_str(), Some(sha.as_str()));
}

#[test]
fn a_subagent_transcript_yields_its_turns_tool_calls_and_active_and_wait_minutes() {
    let text = include_str!("fixtures/subagent.jsonl");
    let lines: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let transcript = super::row::Transcript { lines: &lines, id: "agent-a1", role: "worker", tier: "sonnet", since: "", backfill: false, now: crate::foundation::clock::Timestamp::parse("2026-11-04T12:00:00Z").unwrap() };
    let row = super::row::build(&transcript).unwrap();
    assert_eq!(row["status"], "finished");
    assert_eq!(row["actual_min"], 55);
    assert_eq!(row["turns"], 5);
    assert_eq!(row["tool_calls"], 3);
    assert_eq!(row["active_min"], 13.7);
    assert_eq!(row["wait_min"], 42.0);
}

fn lines(text: &str) -> Vec<Value> {
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_subagent_with_an_agent_type_takes_its_model_from_the_transcript_and_its_effort_from_the_agent_definition() {
    let prices = Prices::parse(CATALOG, DEFAULTS);
    let usage = run_usage(&lines(include_str!("fixtures/identity_subagent.jsonl")), "claude-b", &prices);
    assert_eq!((usage["provider"].as_str(), usage["model"].as_str()), (Some("claude"), Some("claude-sonnet-5-5")));
    assert_eq!(usage["effort"], Value::Null);
    assert_eq!(super::effort(None, Some(&json!({"agentType": "worker-sonnet"})), None, &usage), json!("high"));
    assert_eq!(super::effort(None, Some(&json!({"agentType": "worker-xhigh"})), None, &usage), json!("xhigh"));
    assert_eq!(super::effort(None, Some(&json!({"agentType": "worker", "effort": "low"})), None, &usage), json!("low"));
}

#[test]
fn a_fleet_session_takes_the_requests_logged_effort_over_its_launch_effort_and_a_synthetic_message_keeps_the_real_model() {
    let prices = Prices::parse(CATALOG, DEFAULTS);
    let usage = run_usage(&lines(include_str!("fixtures/identity_fleet.jsonl")), "claude-a", &prices);
    assert_eq!((usage["model"].as_str(), usage["effort"].as_str()), (Some("claude-opus-5-5"), Some("xhigh")));
    assert_eq!(super::effort(None, None, Some("medium"), &usage), json!("xhigh"));
    let unlogged = run_usage(&lines(include_str!("fixtures/identity_subagent.jsonl")), "claude-a", &prices);
    assert_eq!(super::effort(None, None, Some("medium"), &unlogged), json!("medium"));
}

#[test]
fn a_finished_transcript_records_its_brief_text_with_the_transcript_as_its_origin() {
    let lines = lines(include_str!("fixtures/subagent.jsonl"));
    let transcript = super::row::Transcript { lines: &lines, id: "fleet-a1", role: "worker", tier: "sonnet", since: "", backfill: false, now: crate::foundation::clock::Timestamp::parse("2026-11-04T12:00:00Z").unwrap() };
    let row = super::row::build(&transcript).unwrap();
    assert_eq!(row["brief_text"], "Add the thing. ETA 20 minutes. Done when the test passes.");
    assert_eq!(row["brief_origin"], super::row::TRANSCRIPT_ORIGIN);
    assert_eq!(row["eta_min"], 20);
}

#[test]
fn a_transcript_records_each_skill_its_skill_calls_and_the_file_skill_hook_loaded_once_and_none_from_tool_results() {
    let records = [
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Skill", "input": {"skill": "verification"}}, {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}}]}}),
        json!({"type": "attachment", "attachment": {"type": "hook_additional_context", "content": ["file-skill: editing /w/src/main.rs loads the rust-development skill (directory /s/rust-development); its rules apply"]}}),
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Skill", "input": {"skill": "verification"}}]}}),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "file-skill: editing a.ts loads the effect-development skill"}]}}),
    ];
    assert_eq!(super::telemetry::skills(&records), ["rust-development", "verification"]);
    assert!(super::telemetry::skills(&[]).is_empty());
}

#[test]
fn eta_min_records_the_eta_the_brief_head_states_not_the_first_eta_its_text_names() {
    let brief = "Item: north#9 Cut the ETA 5 min sooner\nCategory: tooling\nETA: 126 min (p75 of n=4 runs, cell tooling on sonnet, ledger 7d; 30 min x4.20 eta_pred claude n=300 median 1.10)\nFiles: as north#9 names them\n\nAn ETA of 2 h is a guess.";
    assert_eq!(super::row::eta(brief), json!(126));
    assert_eq!(super::row::eta("Item: north#9 x\nETA: 1.5 h\n"), json!(90));
    assert_eq!(super::row::eta("Fix it. ETA 20 min."), json!(20));
    assert_eq!(super::row::eta("Fix it."), Value::Null);
}

#[test]
fn a_run_whose_safe_push_waited_on_autoland_lands_the_commit_autoland_put_on_main() {
    let landed = "c".repeat(40);
    let records = [
        json!({"type": "assistant", "timestamp": "2026-11-05T09:00:00Z", "message": {"content": [{"type": "tool_use", "id": "c", "name": "Bash", "input": {"command": "git commit -q -m \"Shrink the arcade tick (makgora#1132)\""}}, {"type": "tool_use", "id": "p", "name": "Bash", "input": {"command": "safe-push --to main"}}]}}),
        json!({"type": "user", "timestamp": "2026-11-05T09:20:00Z", "message": {"content": [{"type": "tool_result", "tool_use_id": "c", "content": ""}, {"type": "tool_result", "tool_use_id": "p", "content": format!("safe-push: ✓ Autoland landed this lane: origin main is at {landed} (https://github.com/tompassarelli/makgora/actions/runs/1)")}]}}),
    ];
    let main = MainIndex::parse(&format!("{landed} 2026-11-05T09:19:00Z Shrink the arcade tick, train of two (makgora#1132)\n"));

    assert_eq!(checks(&records, &main)["landed_sha"].as_str(), Some(landed.as_str()));
}

#[test]
fn a_push_logged_without_a_trunk_commit_takes_the_commit_that_reached_main() {
    let (tip, rebased, pinned) = ("4dc485807".to_owned() + &"0".repeat(31), "2d959956d".to_owned() + &"0".repeat(31), "e".repeat(40));
    let main = MainIndex::parse(&format!(
        "{tip} 2026-11-05T09:10:00Z Shrink the arcade tick (makgora#1132)\n{} 2026-11-05T07:00:00Z Share one target dir (north#437)\n{rebased} 2026-11-05T10:05:00Z Share one target dir (north#437)\n{pinned} 2026-11-05T11:00:00Z Later change (north#9)\n",
        "9".repeat(40)
    ));
    let push = |oid: &str, landed: &str, at: &str| Push { at_ms: crate::foundation::clock::Timestamp::parse(at).unwrap().unix_millis() as u64, repository: "north".into(), branch: "lane".into(), oid: oid.into(), landed: landed.into(), session: "s".into() };
    let pushes = vec![
        push(&tip, "", "2026-11-05T09:00:00Z"),
        push(&("546c8bee4".to_owned() + &"1".repeat(31)), "", "2026-11-05T10:00:00Z"),
        push(&("f".repeat(40)), "", "2026-11-05T10:00:00Z"),
        push(&pinned, &pinned, "2026-11-05T11:00:00Z"),
        push(&("a".repeat(40)), "", "2026-11-05T12:00:00Z"),
    ];
    let asked = std::cell::RefCell::new(Vec::new());
    let subject = |push: &Push| {
        asked.borrow_mut().push(push.oid[..9].to_owned());
        push.oid.starts_with("546c8bee4").then(|| "Share one target dir (north#437)".to_owned())
    };

    let landed: Vec<String> = settle(pushes, &main, subject).into_iter().map(|push| push.landed).collect();

    assert_eq!(landed, [tip, rebased, String::new(), pinned, String::new()]);
    assert_eq!(*asked.borrow(), ["546c8bee4", "fffffffff"], "only a tip off main pushed before main's newest commit is looked up");
}

#[test]
fn a_lane_landed_in_a_batch_takes_its_own_commit_not_the_batch_tip_safe_push_logged() {
    let (own, other, batch) = ("1".repeat(40), "2".repeat(40), "3".repeat(40));
    let main = MainIndex::parse(&format!(
        "{own} 2026-11-05T09:01:00Z Pin the offload runtime (north#482)\n{other} 2026-11-05T09:01:00Z Watch the canary (north#482)\n{batch} 2026-11-05T09:01:00Z Add north digest (north#522)\n"
    ));
    let push = |oid: &str| Push { at_ms: crate::foundation::clock::Timestamp::parse("2026-11-05T09:00:00Z").unwrap().unix_millis() as u64, repository: "north".into(), branch: "lane".into(), oid: oid.into(), landed: batch.clone(), session: "s".into() };
    let subject = |push: &Push| match &push.oid[..1] {
        "a" => Some("Pin the offload runtime (north#482)".to_owned()),
        "b" => Some("Watch the canary (north#482)".to_owned()),
        _ => None,
    };

    let landed: Vec<String> = settle(vec![push(&"a".repeat(40)), push(&"b".repeat(40)), push(&"c".repeat(40)), push(&batch)], &main, subject).into_iter().map(|push| push.landed).collect();

    assert_eq!(landed, [own, other, batch.clone(), batch]);
}

#[test]
fn a_lane_the_lander_cherry_picked_before_its_checks_lands_with_that_commit_though_it_predates_the_logged_push() {
    let (earlier, own, batch, later) = ("4".repeat(40), "5".repeat(40), "6".repeat(40), "7".repeat(40));
    let main = MainIndex::parse(&format!(
        "{earlier} 2026-11-05T07:00:00Z Pin the offload runtime (north#482)\n{own} 2026-11-05T08:40:00Z Pin the offload runtime (north#482)\n{batch} 2026-11-05T08:41:00Z Add north digest (north#522)\n{later} 2026-11-05T12:00:00Z Pin the offload runtime (north#482)\n"
    ));
    let at = |at: &str| crate::foundation::clock::Timestamp::parse(at).unwrap().unix_millis() as u64;
    let batched = Push { at_ms: at("2026-11-05T09:01:00Z"), repository: "north".into(), branch: "offload-runtime-pin".into(), oid: "a".repeat(40), landed: batch, session: "s".into() };
    let elsewhere = Push { landed: String::new(), at_ms: at("2026-11-05T11:00:00Z"), ..batched.clone() };
    let subject = |_: &Push| Some("Pin the offload runtime (north#482)".to_owned());

    let landed: Vec<String> = settle(vec![batched, elsewhere], &main, subject).into_iter().map(|push| push.landed).collect();

    assert_eq!(landed, [own, later], "a logged trunk commit bounds the lane's own commit from above; without one the first after the push is the lane's");
}
