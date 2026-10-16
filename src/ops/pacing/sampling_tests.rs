use super::sampling;
use serde_json::{Value, json};

#[test]
fn recorded_usage_samples_keep_account_controls_and_gate_in_agreement() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/sampling-replay.json")).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let mut state = case["before"].clone();
        let mut gate = json!({"codex":{"projections":case["windows"]}});
        sampling::update_controls(
            &mut state,
            &mut gate,
            &["codex"],
            case["nodes"].as_array().unwrap(),
            case["at"].as_f64().unwrap(),
            2,
            |_, _| Ok(None),
        )
        .unwrap();
        assert_eq!(state["accounts"], case["accounts"], "sample state {index}");
        assert_eq!(
            gate["codex"]["controls"], case["gate_controls"],
            "sample gate {index}"
        );
    }
}
