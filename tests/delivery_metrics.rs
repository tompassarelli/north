use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const DRIVER: &str = "import json, sys\nsys.path.insert(0, sys.argv[1])\nimport ledger_context\n\
rows = json.loads(sys.argv[3])\nprint(json.dumps(ledger_context.delivery_metrics(rows, sys.argv[2])))";

fn run(agent: &str, item: &str, cost: f64) -> Value {
    json!({"agent": agent, "item": item, "role": "worker", "outcome": "done", "landed": 1, "follows": null,
           "staffed": "2026-10-11T00:00:00Z", "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:20:00Z",
           "landed_at": "2026-10-11T00:20:00Z", "cost_usd": cost})
}

fn metrics(rows: &[Value]) -> Value {
    let output = Command::new("python3")
        .args(["-I", "-c", DRIVER])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/lib"))
        .args(["north", &Value::from(rows).to_string()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn delivery_metrics_count_done_boxes_concurrency_landing_time_and_box_cost() {
    let rows = [run("a", "north#2", 1.46), run("b", "wisp#3", 3.54)];
    let alone = metrics(&rows);
    assert_eq!(alone["done_boxes"], 1);
    assert_eq!(alone["concurrency"], 1);
    assert_eq!(alone["median_brief_to_landed_min"].as_f64(), Some(20.0));
    assert_eq!(alone["median_cost_per_done_usd"].as_f64(), Some(1.46));

    let mut retry = run("a0", "north#2", 0.54);
    retry.as_object_mut().unwrap().extend([
        ("outcome".to_owned(), json!("not done")),
        ("landed".to_owned(), json!(0)),
        ("landed_at".to_owned(), Value::Null),
        ("staffed".to_owned(), json!("2026-10-10T23:40:00Z")),
        ("started".to_owned(), json!("2026-10-10T23:40:00Z")),
        ("ended".to_owned(), json!("2026-10-10T23:55:00Z")),
    ]);
    let with_retry = metrics(&[rows[0].clone(), rows[1].clone(), retry]);
    assert_eq!(with_retry["median_cost_per_done_usd"].as_f64(), Some(2.0));
}
