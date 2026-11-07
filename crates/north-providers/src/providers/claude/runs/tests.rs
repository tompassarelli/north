
use serde_json::{Value, json};

use super::landing::{MainIndex, checks, settle};
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
