
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

