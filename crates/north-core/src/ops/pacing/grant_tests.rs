use super::{grants, reservation, storage};
use serde_json::{Value, json};

#[test]
fn recorded_grants_reserve_uses_cancel_failures_and_confirm_once_after_expiry() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/grant-replay.json")).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        storage::save(&path.join("pace-control.json"), &case["before"]).unwrap();
        storage::save(&path.join("usage-gate.json"), &case["gate"]).unwrap();
        if let Some(audit) = case["before_audit"].as_array() {
            for row in audit {
                storage::append(&path.join("pace-control.jsonl"), row).unwrap();
            }
        }
        let mut request = case["request"].clone();
        let at = case["at"].as_f64().unwrap();
        let nodes = case["nodes"].as_array().unwrap();
        let result = match case["op"].as_str().unwrap() {
            "grant" => {
                request["max_workers"] = request["maximum"].clone();
                grants::create(path, &request, nodes, at, Some("replay-operator")).unwrap()
            }
            "grant_for" => json!(grants::find(
                &case["before"],
                request["account"].as_str().unwrap(),
                &request["session"],
                &request["task"],
                at,
                nodes
            )),
            "reserve" => json!(
                reservation::reserve(&path.join("usage-gate.json"), &request, nodes, || at)
                    .unwrap()
            ),
            "reserve_after_lock" => json!(
                reservation::reserve(&path.join("usage-gate.json"), &request, nodes, || {
                    let lock = storage::lock(path, "pace-control.lock", Some(0.0));
                    if lock.is_err() {
                        case["after_lock_at"].as_f64().unwrap()
                    } else {
                        at
                    }
                })
                .unwrap()
            ),
            "expired" => {
                grants::expired(path, &request, nodes, at).unwrap();
                Value::Null
            }
            "confirm" => {
                grants::confirm(
                    path,
                    &request["session"],
                    &request["tool_use_id"],
                    request["launched"].as_bool().unwrap_or(true),
                    || at,
                )
                .unwrap();
                Value::Null
            }
            other => panic!("unrecognized source operation {other}"),
        };
        if let Some(expected) = case.get("after_audit") {
            let audit: Vec<Value> = std::fs::read_to_string(path.join("pace-control.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(json!(audit), *expected, "audit at source operation {index}");
        }
        assert_eq!(result, case["result"], "result at source operation {index}");
        assert_eq!(
            storage::load(&path.join("pace-control.json")),
            case["after"],
            "state at source operation {index}"
        );
        assert_eq!(
            storage::load(&path.join("usage-gate.json")),
            case["after_gate"],
            "gate at source operation {index}"
        );
    }
}

#[test]
fn concurrent_reservations_cannot_spend_the_same_remaining_grant_use() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let at = 1800000000.0;
    let nodes =
        vec![json!({"name":"lead","conversation":"parent","provider":"claude","status":"working"})];
    let grant = json!({"account":"claude:a","lead":"lead","session":"parent","until":at+3600.0,"at":at,"uses":1,"task":"review","max_workers":8,"authority":"proxy"});
    storage::save(&path.join("pace-control.json"), &json!({"grants":[grant]})).unwrap();
    let admitted = std::thread::scope(|scope| {
        let jobs:Vec<_>=(0..8).map(|id|{let nodes=&nodes;scope.spawn(move || {
            let request=json!({"provider":"claude","account":"claude:a","session":"parent","task":"review","tool_use_id":format!("tool-{id}")});
            reservation::reserve(&path.join("usage-gate.json"),&request,nodes,||at).unwrap().is_none()
        })}).collect();
        jobs.into_iter()
            .map(|job| usize::from(job.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(admitted, 1);
    assert_eq!(
        storage::load(&path.join("pace-control.json"))["grants"][0]["pending"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_grant_names_a_claude_account_orchestration_configures_and_nothing_else() {
    let cfg = "[accounts.claude]\nplans = { a = \"max-20x\", d = \"max-5x\" }\n[accounts.claude.profiles.c]\nhere = {}\n".parse().unwrap();
    let request = |account: &str| super::command::grant_request(&["grant".into(), account.into(), "2".into()], &cfg);
    for account in ["claude:a", "claude:c", "claude:d"] {
        assert_eq!(request(account).unwrap()["account"], account);
    }
    for account in ["claude:", "claude:z", "claude:dd", "claude:../a", "codex"] {
        assert_eq!(request(account), Err("invalid grant account".into()), "{account}");
    }
}
