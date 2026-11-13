use super::super::model::{LaunchRequest, SessionRecord};

fn launched(provider: &str) -> SessionRecord {
    let request: LaunchRequest = serde_json::from_value(serde_json::json!({
        "verification": false, "name": "w", "provider": provider, "account": "a", "cwd": "/tmp", "model": null, "prompt": "", "system_prompt": null,
        "foreground_cap_secs": null, "prompt_mode": "north", "idle_exit_minutes": null, "parent": null, "task_ref": null, "placement": null, "machine": null
    }))
    .unwrap();
    request.record(None, None, false, 0)
}

#[test]
fn a_claude_run_record_carries_the_idle_compaction_flag_and_another_provider_carries_none() {
    let claude = serde_json::to_value(launched("claude")).unwrap();
    assert_eq!(claude["idle_compaction"], true);
    assert!(serde_json::to_value(launched("codex")).unwrap().get("idle_compaction").is_none());
}
