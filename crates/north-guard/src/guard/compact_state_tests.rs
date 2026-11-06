use super::*;

fn record() -> SessionRecord {
    serde_json::from_value(serde_json::json!({"name": "n636-w", "provider": "claude", "account": "a", "cwd": "/lane", "model": null, "created_ms": 1, "host_pid": null, "task_ref": "north#636"})).unwrap()
}

#[test]
fn a_compacted_session_is_told_the_brief_it_launched_with_from_its_transcript_with_its_item_line_lane_path_open_boxes_and_waits() {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("c.jsonl");
    let lines = [
        r#"{"type":"system","subtype":"init"}"#,
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>x</local-command-caveat>"}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Item: north#636 The worker decides when to compact\nGoal: build the checkpoint"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":"a later message"}}"#,
    ];
    std::fs::write(&transcript, lines.join("\n")).unwrap();
    let state = State {
        brief: first_prompt(&transcript),
        lane: Lane { path: "/home/tom/code/north/worktrees/n636-ckpt".into(), branch: Some("n636-ckpt".into()), unlanded: vec!["abc1234 Add the checkpoint".into()], dirty_files: 2 },
        open_boxes: unticked("- [x] done\n- [ ] A compaction keeps the brief\n  - [ ] nested\n- [ ] Calibrated"),
        waits: vec!["job:cargo-test (full suite)".into()],
    };

    let text = state.render(&record());

    assert!(text.contains("Item: north#636 The worker decides when to compact\nGoal: build the checkpoint") && !text.contains("a later message"), "{text}");
    assert!(text.contains("Your lane: /home/tom/code/north/worktrees/n636-ckpt, branch n636-ckpt, 2 uncommitted file(s)."), "{text}");
    assert!(text.contains("- abc1234 Add the checkpoint"), "{text}");
    assert!(text.contains("- [ ] A compaction keeps the brief") && text.contains("- [ ] nested") && !text.contains("- [x] done"), "{text}");
    assert!(text.contains("- job:cargo-test (full suite)"), "{text}");
    assert_eq!(clip(&"x".repeat(20), 5), "xxxxx…");
    let startup = HookInput::parse(br#"{"hook_event_name":"SessionStart","source":"startup","session_id":"c","cwd":"/tmp"}"#).unwrap();
    assert!(matches!(evaluate(&startup), Verdict::Allow));
}
