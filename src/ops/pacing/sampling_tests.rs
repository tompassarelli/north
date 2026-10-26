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

#[test]
fn claude_usage_is_also_read_through_ssh_on_each_other_host_holding_a_claude_profile() {
    let cfg = "[hosts.far]\nssh = \"tom@far\"\n[hosts.idle]\nssh = \"tom@idle\"\n[accounts.claude]\nprofiles = { a = { here = {} }, b = { far = {} }, c = { far = {}, here = {} } }\n".parse().unwrap();
    assert_eq!(sampling::claude_owners(&cfg, "here"), ["tom@far"]);
}

#[test]
fn a_provider_is_read_here_only_when_its_accounts_table_names_this_host_or_names_no_hosts() {
    let cfg = "[accounts.claude]\nprofiles = { a = { here = {} }, b = { far = {} } }\n[accounts.codex]\ntarget_left = 0\n".parse().unwrap();
    let read = |provider, host| sampling::read_here(&cfg, provider, host);
    assert_eq!([read("claude", "here"), read("claude", "far"), read("claude", "idle")], [true, true, false]);
    assert_eq!([read("codex", "idle"), read("gemini", "here")], [true, false]);
}
