
use serde_json::{Value, json};

use super::landing::{MainIndex, checks, settle};
use super::telemetry::Prices;
use crate::fleet::landings::Push;

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
        vec![json!({"type": "attachment", "timestamp": "2026-10-29T16:07:37Z", "attachment": {"type": "queued_command", "commandMode": "task-notification", "prompt": format!("<task-notification>\n<summary>Monitor event: \"safe-push landing\"</summary>\n<event>safe-push: landed {own}</event>\n</task-notification>")}})],
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
fn a_subagent_transcript_adds_its_spend_to_its_parent_run() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("session.jsonl");
    let agents = directory.path().join("session/subagents");
    std::fs::create_dir_all(&agents).unwrap();
    let request = |id: &str, model: &str| {
        json!({"type": "assistant", "requestId": id, "timestamp": "2026-11-06T12:00:00Z", "message": {"model": model, "usage": {"input_tokens": 1_000_000, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}})
    };
    let parent_lines = vec![request("parent-request", "claude-sonnet-5-5")];
    std::fs::write(agents.join("agent-explore.jsonl"), format!("{}\n", request("explore-request", "claude-haiku-5-5"))).unwrap();
    std::fs::write(agents.join("agent-explore.meta.json"), json!({"agentType": "Explore"}).to_string()).unwrap();
    let prices = Prices::parse(&super::CATALOG, &super::DEFAULTS);

    let usage = super::usage_for_run(&parent_lines, &parent, "a", &prices, &|_| false);

    let requests: Vec<&str> = usage["usage_samples"].as_array().unwrap().iter().filter_map(|sample| sample["request"].as_str()).collect();
    assert_eq!(requests, ["parent-request", "explore-request"]);
    assert!(usage["usage_samples"][1]["weight"].as_f64().is_some_and(|weight| weight > 0.0));
}

#[test]
fn a_request_is_weighted_at_list_price_with_its_one_hour_and_five_minute_writes_split_from_cache_creation() {
    let prices = super::telemetry::Prices::parse(&super::CATALOG, &super::DEFAULTS);
    let request = |id: &str, model: &str, short: i64, long: i64, read: i64| {
        let usage = json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": read, "cache_creation_input_tokens": short + long, "cache_creation": {"ephemeral_5m_input_tokens": short, "ephemeral_1h_input_tokens": long}});
        json!({"type": "assistant", "requestId": id, "timestamp": "2026-11-06T12:00:00Z", "message": {"model": model, "usage": usage}})
    };
    let weights = |records: &[Value]| -> Vec<f64> {
        let usage = super::telemetry::run_usage(records, "c", &prices);
        usage["usage_samples"].as_array().unwrap().iter().map(|sample| (sample["weight"].as_f64().unwrap() / 1000.0).round() / 1000.0).collect()
    };
    let million = 1_000_000;
    assert_eq!(weights(&[request("a", "claude-opus-5-5", 0, million, 0), request("b", "claude-opus-5-5", million, 0, 0), request("c", "claude-opus-5-5", 0, 0, million)]), [8.0, 5.0, 0.2]);
    assert_eq!(weights(&[request("d", "claude-sonnet-5-5", 0, million, million)]), [4.2]);
    assert_eq!(weights(&[request("e", "claude-haiku-5-5", 0, million, million)]), [0.21]);
}

#[test]
fn a_resume_past_the_cache_ttl_counts_as_a_miss_when_it_rewrites_at_least_half_its_context_though_the_system_prefix_stays_cached() {
    let request = |id: &str, at: &str, write: i64, read: i64| {
        let usage = json!({"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": read, "cache_creation_input_tokens": write, "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": write}});
        json!({"type": "assistant", "requestId": id, "timestamp": at, "message": {"model": "claude-opus-5-5", "usage": usage}})
    };
    let records = [
        request("a", "2026-11-06T10:00:00Z", 100_000, 0),
        request("b", "2026-11-06T10:05:00Z", 2_000, 100_000),
        request("c", "2026-11-06T11:30:00Z", 80_000, 22_000),
        request("d", "2026-11-06T13:00:00Z", 5_000, 97_000),
        request("e", "2026-11-06T13:10:00Z", 60_000, 50_000),
        request("f", "2026-11-06T15:00:00Z", 110_000, 0),
    ];
    assert_eq!(super::telemetry::context_metrics(&records)["idle_gap_misses"], json!(2));
}

#[test]
fn the_runs_of_a_worker_on_the_api_account_are_read_from_its_declared_config_dir_north_748() {
    let home = tempfile::tempdir().unwrap();
    let profiles = home.path().join(".local/state/north/profiles");
    let policy: toml_edit::DocumentMut = "[accounts.claude.profiles.a]\nlaptop = { path = \"~/.local/state/north/profiles/claude-a\", token_file = \"~/tokens/claude-a\" }\n[accounts.claude.api_keys.api]\nlaptop = { path = \"~/.local/state/north/profiles/claude-api\" }\nother = { path = \"~/elsewhere\" }\n".parse().unwrap();
    for profile in ["claude-a", "claude-api", "claude-z"] {
        std::fs::create_dir_all(profiles.join(profile).join("projects/-home-tom-code-north")).unwrap();
        std::fs::write(profiles.join(profile).join("projects/-home-tom-code-north/run.jsonl"), "{}\n").unwrap();
    }
    let roots = super::roots(home.path(), None, Some(&policy), "laptop");
    let read: Vec<(std::path::PathBuf, String)> = roots.iter().map(|root| (root.projects.clone(), root.account.clone())).collect();
    assert!(read.contains(&(profiles.join("claude-api/projects"), "claude-api".to_owned())), "{read:?}");
    assert!(read.contains(&(profiles.join("claude-a/projects"), "claude-a".to_owned())), "{read:?}");
    assert!(!read.iter().any(|(projects, _)| projects.starts_with(home.path().join("elsewhere"))), "{read:?}");
    let options = super::Options { session: None, since: String::new(), settled: Default::default(), recorded: Default::default() };
    assert_eq!(super::unread(&profiles, &roots, &options), vec![profiles.join("claude-z")]);
}

#[test]
fn a_turn_north_interrupts_mid_work_scores_idle_so_the_turn_after_it_decides_the_run() {
    let at = |n: u32| format!("2026-11-07T10:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let lines = vec![
        text("user", 1, "Item: north#757"),
        text("assistant", 2, "Writing the lifecycle module (state.rs, then the tests)."),
        text("user", 30, "[Request interrupted by user]"),
        text("user", 31, "[from proxy] a message mid-work"),
        text("assistant", 32, "Noted, carrying on."),
    ];
    let transcript = super::row::Transcript { lines: &lines, id: "t", role: "worker", tier: "sonnet", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) };

    assert_eq!(super::row::build(&transcript).unwrap()["outcome"], "idle");
}

#[test]
fn a_reply_to_north_s_spend_nudge_that_ends_a_working_turn_scores_idle_gex45_n757_gates() {
    let at = |n: u32| format!("2026-11-07T10:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let lines = vec![
        text("user", 52, "Item: north#757"),
        json!({"type": "user", "timestamp": at(57), "sessionId": "s", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "u1", "content": "Command running in background with ID: bprov757."}]}}),
        text("assistant", 58, "The providers tests are running in the background."),
        text("user", 59, "[from service:north-budget@gex45] [north: budget] north#757 has spent $2.49 against E p50 $2.76, p90 $9.00, past half of p50 in under a quarter of the expected minutes; check your approach still fits the box's budget."),
        text("assistant", 59, "Spend is $2.49 against the $2.76 p50 and the approach still fits. The remaining work is the providers test result, clippy, commit and landing."),
    ];
    let transcript = super::row::Transcript { lines: &lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) };

    assert_eq!(super::row::build(&transcript).unwrap()["outcome"], "idle");
}

#[test]
fn a_turn_that_ends_while_its_background_command_runs_scores_idle_north_725_d0351() {
    let at = |n: u32| format!("2026-11-07T10:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let started = json!({"type": "user", "timestamp": at(54), "sessionId": "s", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "u1", "content": "Command running in background with ID: b2poll. Output is being written to: /tmp/b2poll.output."}]}});
    let mut lines = vec![text("user", 51, "Item: north#725"), started, text("assistant", 56, "Not landed yet (empty output); the second poll is still running.")];
    let transcript = |lines: &[serde_json::Value]| super::row::build(&super::row::Transcript { lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) }).unwrap()["outcome"].clone();

    assert_eq!(transcript(&lines), "idle");
    lines.push(text("user", 58, "<task-notification><task-id>b2poll</task-id><status>completed</status></task-notification>"));
    lines.push(text("assistant", 59, "Reading the poll output next."));
    assert_eq!(transcript(&lines), "not done", "once its command has ended, the turn that ended on its own stands until the ledger settles it, and the wake-up that ran no tool does not replace it");
}

#[test]
fn a_not_done_report_written_while_a_background_command_still_runs_scores_idle_gex45_n757_leases() {
    let at = |n: u32| format!("2026-11-07T11:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let started = json!({"type": "user", "timestamp": at(12), "sessionId": "s", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "u1", "content": "Command running in background with ID: bclippy7."}]}});
    let lines = vec![text("user", 5, "Item: north#757"), started, text("assistant", 14, "Clippy and the `lease_lifecycle` test run are queued in admission, and their end will notify me. Not done: still unlanded. Stop: capacity")];
    let transcript = super::row::Transcript { lines: &lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) };

    assert_eq!(super::row::build(&transcript).unwrap()["outcome"], "idle");
}

#[test]
fn a_report_written_while_its_own_job_run_still_runs_scores_idle_north_730_d0351() {
    let at = |n: u32| format!("2026-11-07T12:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let started = json!({"type": "user", "timestamp": at(20), "sessionId": "s", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "u1", "content": "job-run: n730-freeze-landed runs detached with its output in /tmp/n730.log; its exit arrives as a fleet message to north-730-d0351, so end your turn."}]}});
    let mut lines = vec![text("user", 10, "Item: north#730"), started, text("assistant", 29, "Not done: the freeze commit e4df7cd08 is queued with the lander. The `n730-freeze-landed` job will message me once it is on origin/main.\nStop: wait until 2026-11-07T06:30Z")];
    let transcript = |lines: &[serde_json::Value]| super::row::build(&super::row::Transcript { lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) }).unwrap()["outcome"].clone();

    assert_eq!(transcript(&lines), "idle");
    lines.push(text("user", 40, "[from service:north-job@gex45] job-run: n730-freeze-landed exited 0; output in /tmp/n730.log"));
    lines.push(text("assistant", 41, "Reading the job output next."));
    assert_eq!(transcript(&lines), "not done", "once its job has exited, the Not done report stands");
}

#[test]
fn a_turn_a_wait_wake_up_opened_keeps_the_split_reported_before_it_north_636_d1824() {
    let at = |n: u32| format!("2026-11-07T18:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let lines = vec![
        text("user", 25, "Item: north#636"),
        text("assistant", 26, "Split: I ticked one more Done-when box on north#636 with evidence. The other 17 open boxes are separate pieces of work, each already listed on north#636 for dispatch."),
        text("user", 27, "[from service:north-fleet] [north: the run you waited on is over] pid:117681@27250137 `north check privacy judge --text -`: ended. Read its output and continue to a final report."),
        text("assistant", 28, "Done: that notice is from the privacy check that ran when I edited the north#636 body, and the edit went through. The Split report stands."),
    ];
    let outcome = |lines: &[serde_json::Value]| super::row::build(&super::row::Transcript { lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) }).unwrap();

    assert_eq!(outcome(&lines)["outcome"], "not done", "the ledger settles a split from its records, not its opener");
    assert!(outcome(&lines)["report"].as_str().unwrap().starts_with("Split:"), "the wake-up that ran no tool keeps the report before it");
}

#[test]
fn a_turn_a_landing_notice_opened_that_ticks_the_box_replaces_the_not_done_before_it_bd_706_midturn() {
    let at = |n: u32| format!("2026-11-08T03:{n:02}:00Z");
    let text = |kind: &str, n: u32, text: &str| json!({"type": kind, "timestamp": at(n), "sessionId": "s", "message": {"role": kind, "model": "m", "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let tick = json!({"type": "assistant", "timestamp": at(42), "sessionId": "s", "message": {"role": "assistant", "model": "m", "content": [{"type": "tool_use", "id": "e", "name": "Bash", "input": {"command": "gh issue edit 706 --body-file /tmp/body.md"}}], "usage": {"input_tokens": 1, "output_tokens": 1}}});
    let lines = vec![
        text("user", 10, "Item: north#706"),
        text("assistant", 30, "Not done: the fix is written, tested and queued with the lander, but it is not on origin/main yet.\nStop: landing"),
        text("user", 40, "<task-notification>\n<task-id>bsv7iuuvp</task-id>\n<summary>Monitor event: \"lane on origin/main\"</summary>\n</task-notification>"),
        tick,
        text("assistant", 43, "Done: my box on north#706 is ticked. The fix landed as `db1ea6343d`, an ancestor of origin/main."),
    ];
    let transcript = super::row::Transcript { lines: &lines, id: "t", role: "worker", tier: "opus", since: "", backfill: false, now: crate::foundation::clock::Timestamp::from_unix_seconds(1_800_000_000) };

    let row = super::row::build(&transcript).unwrap();
    assert_eq!(row["outcome"], "not done", "the ledger settles it done from its landing");
    assert!(row["report"].as_str().unwrap().starts_with("Done: my box"), "the woken turn that did work replaces the Not done before it");
}

#[test]
fn a_habit_rewrite_and_warning_are_recorded_per_rule_at_the_edit() {
    let call = json!({"type": "assistant", "timestamp": "2026-11-09T10:00:00Z", "message": {"content": [{"type": "tool_use", "id": "e1", "name": "Edit", "input": {"file_path": "tests/x.rs"}}]}});
    let context = "\
code-health: this edit adds structural debt; fix it now\n\
habits: ambient-test-input rewrote tests/x.rs:3 for you. Before: `let old = SystemTime::now() - d;` After: `let old = UNIX_EPOCH - d;` Write this form next time.\n\
habits: fix these before your next commit; they have no mechanical fix, and safe-push refuses a lane that still has one:\n\
ambient-test-input tests/x.rs:7 thread::sleep in test code reads the wall clock or orders events by sleeping\n\
shared-mutability tests/x.rs:9 RefCell in non-test code";
    let attachment = json!({"type": "attachment", "timestamp": "2026-11-09T10:00:01Z", "attachment": {"type": "hook_additional_context", "hookEvent": "PostToolUse", "toolUseID": "e1", "hookName": "north-hook:Edit", "content": [context]}});

    let rows = super::hook_events::events("s", &[call, attachment]);
    let shown: Vec<(&str, &str)> = rows.iter().map(|row| (row["verdict"].as_str().unwrap(), row["rule"].as_str().unwrap())).collect();
    assert_eq!(shown, [("rewrite", "ambient-test-input"), ("advisory", "ambient-test-input"), ("advisory", "shared-mutability"), ("advisory", "code-health")]);
}

#[test]
fn a_run_in_a_scratch_directory_that_landed_a_commit_is_stored_as_work_not_a_probe_north_1103() {
    let project = std::path::Path::new("-tmp-north-verify-north-1794324732-lane/0f1e.jsonl");
    let landed = serde_json::Map::from_iter([("landed".to_owned(), json!(1)), ("landed_sha".to_owned(), json!("72f618c352e39e967c72e473ddb57b5db9f9e803"))]);
    assert!(!super::scratch_probe(&landed, project), "on 2026-11-10 verify-north's worker landed 72f618c352 from a scratch lane, was stored as a probe, and the settle left it not done");
    assert!(super::scratch_probe(&serde_json::Map::from_iter([("landed".to_owned(), json!(0))]), project));
}
