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
    let cfg = "[accounts.claude]\nprofiles = { a = { here = {} }, b = { far = {} } }\n[accounts.gemini]\ntarget_left = 0\n".parse().unwrap();
    let read = |provider, host| sampling::read_here(&cfg, provider, host);
    assert_eq!([read("claude", "here"), read("claude", "far"), read("claude", "idle")], [true, true, false]);
    assert_eq!([read("gemini", "idle"), read("retired", "here")], [true, false]);
}

#[test]
fn the_host_holding_threads_db_keeps_ten_minutes_of_pace_history_and_a_snapshot_host_three_hours() {
    let at = |minutes_ago: f64| {
        let ts = crate::foundation::clock::Timestamp::from_unix_seconds_f64(super::storage::now() - minutes_ago * 60.0).iso();
        json!({"ts": ts, "provider": "claude", "account": "a", "kind": "window", "name": "session", "used_pct": 1})
    };
    let state = json!({"history": [at(150.0), at(20.0), at(5.0)]});
    let kept = |remote| sampling::recent_history(&state, &[at(0.0)], sampling::history_window(remote)).len();
    assert_eq!((kept(false), kept(true)), (2, 4));
}

#[test]
fn a_pace_hold_or_resume_reaches_the_phone_without_naming_a_spawn_stop() {
    for held in [true, false] {
        let control = serde_json::from_value(json!({"held": held})).unwrap();
        let (title, line) = sampling::phone_notice("claude:c", &control);
        for text in [&title, &line] {
            assert!(!text.contains("SPAWNS"), "{text}");
            assert!(text.contains("claude:c"), "{text}");
        }
        assert_eq!(line.starts_with("PACE-HELD"), held, "{line}");
    }
}

#[test]
fn a_provider_without_an_adapter_is_dropped_from_the_saved_pace_state() {
    let mut state = json!({
        "accounts": {"claude:a": {}, "codex": {}},
        "providers": {"claude": {}, "codex": {}},
        "history": [{"provider": "claude"}, {"provider": "codex"}],
        "grants": [{"account": "claude:b"}, {"account": "codex"}],
    });
    sampling::prune_retired(&mut state);
    assert_eq!(
        state,
        json!({
            "accounts": {"claude:a": {}},
            "providers": {"claude": {}},
            "history": [{"provider": "claude"}],
            "grants": [{"account": "claude:b"}],
        })
    );
}

#[test]
fn a_token_only_accounts_rows_join_the_claude_read_when_claude_is_due_and_are_dropped_when_it_is_not() {
    let row = json!({"provider": "claude", "account": "d", "kind": "window", "name": "session", "used_pct": 37});
    let mut due = sampling::Reads::from([("claude", (vec![json!({"account": "a"})], json!({"code": 0})))]);
    sampling::merge_token_rows(&mut due, || vec![row.clone()]);
    assert_eq!(due["claude"].0.iter().map(|row| row["account"].as_str().unwrap()).collect::<Vec<_>>(), ["a", "d"]);
    let mut idle = sampling::Reads::from([("gemini", (Vec::new(), json!({"code": 0})))]);
    sampling::merge_token_rows(&mut idle, || unreachable!("no request goes out while claude is not due"));
    assert!(idle["gemini"].0.is_empty() && !idle.contains_key("claude"));
}

#[test]
fn the_claude_cli_refresh_is_skipped_only_when_every_declared_account_has_a_token_on_this_host() {
    let covered = |profiles: &str| sampling::tokens_cover_claude(&format!("[accounts.claude]\nprofiles = {{ {profiles} }}\n").parse().unwrap(), "here");
    assert!(covered("a = { here = { path = \"~/a\", token_file = \"~/ta\" } }, d = { here = { token_file = \"~/td\" }, far = {} }"));
    assert!(!covered("a = { here = { path = \"~/a\", token_file = \"~/ta\" } }, b = { here = { path = \"~/b\" } }"));
    assert!(!covered("a = { here = { token_file = \"~/ta\" } }, b = { far = { token_file = \"~/tb\" } }"));
    assert!(!covered(""));
}
