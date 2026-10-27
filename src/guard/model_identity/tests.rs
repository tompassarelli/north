use super::*;
use serde_json::{Value, json};

struct Records {
    _temporary: tempfile::TempDir,
    root: std::path::PathBuf,
    context: Context,
}

fn records() -> Records {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let root = home.join("code/todo");
    std::fs::create_dir_all(&root).unwrap();
    let context = Context::new(home.to_str().unwrap(), temporary.path().to_str().unwrap(), root.to_str().unwrap());
    Records { _temporary: temporary, root, context }
}

impl Records {
    fn path(&self, name: &str) -> String {
        self.root.join(name).display().to_string()
    }

    fn denies(&self, payload: Value) -> bool {
        let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
        self.context.decide(&input)
    }

    fn bash(&self, command: &str) -> bool {
        self.denies(json!({"tool_name": "Bash", "tool_input": {"command": command}}))
    }
}

#[test]
fn todo_record_boundary_denies_placeholders_across_tools_and_ignores_other_files() {
    let records = records();
    let path = records.path("task.md");
    for (tool, field) in [("Write", "content"), ("Edit", "new_string"), ("MultiEdit", "edits")] {
        for (model, denied) in [("inherited", true), ("gpt-6-sol", false)] {
            let text = format!("model = \"{model}\"");
            let (key, value) = if tool == "MultiEdit" {
                ("filePath", json!([{"newString": text}]))
            } else {
                ("file_path", json!(text))
            };
            let payload = json!({"tool_name": tool, "tool_input": {key: path, field: value}});
            assert_eq!(records.denies(payload.clone()), denied, "{payload}");
        }
    }
    patch_and_shell_writes_to_a_todo_record_are_checked();
    only_records_directly_inside_the_todo_root_are_checked();
    unrelated_tools_and_shapes_pass();
}

fn patch_and_shell_writes_to_a_todo_record_are_checked() {
    let records = records();
    let path = records.path("task.md");
    let patch = format!("*** Begin Patch\n*** Add File: {path}\n+model = \"self\"\n*** End Patch");
    assert!(records.denies(json!({"tool_name": "apply_patch", "tool_input": {"input": patch}})));
    assert!(records.bash(&format!("printf 'model = \"self\"' > {path}")));
    assert!(!records.bash(&format!("printf 'model = \"gpt-6-sol\"' > {path}")));
    assert!(!records.bash("printf 'model = \"self\"' > /tmp/elsewhere.md"));
}

fn only_records_directly_inside_the_todo_root_are_checked() {
    let records = records();
    let other = records._temporary.path().join("other.md");
    let payload = json!({"tool_name": "Write", "tool_input": {"file_path": other, "content": "model = \"self\""}});
    assert!(!records.denies(payload));
    let text = json!({"tool_name": "Write", "tool_input": {"file_path": records.path("note.txt"), "content": "model = \"self\""}});
    assert!(!records.denies(text));
}

#[test]
fn calibration_and_ledger_records_reject_placeholder_models_in_every_form() {
    ledger_rows_need_a_concrete_model_in_the_third_column_and_headings_pass();
    let records = records();
    let path = records.path("estimate-calibration.md");
    for (content, denied) in [
        ("model: self", true),
        ("model: claude-opus-5-5", false),
        ("reviewer model: none", false),
        ("reviewer model: parent", true),
        ("task; model=auto", true),
        ("task | model = gpt-6-sol", false),
        ("plan — one parent", true),
        ("plan — gpt-6-sol", false),
        ("| actor | note |\n| --- | --- |\n| inherited | x |", true),
        ("| actor | note |\n| --- | --- |\n| claude-opus-5-5 inherited | x |", false),
        ("| other | note |\n| --- | --- |\n| inherited | x |", false),
    ] {
        let payload = json!({"tool_name": "Write", "tool_input": {"file_path": path, "content": content}});
        assert_eq!(records.denies(payload), denied, "{content}");
    }
}

fn ledger_rows_need_a_concrete_model_in_the_third_column_and_headings_pass() {
    let records = records();
    let path = records.path("model-assignment-ledger.md");
    for (content, denied) in [
        ("| 2026-10-27 | inherited | x | y |", true),
        ("| 2026-10-27 | gpt-6-sol | x | y |", false),
        ("# model = inherited heading", false),
    ] {
        let payload = json!({"tool_name": "Write", "tool_input": {"file_path": path, "content": content}});
        assert_eq!(records.denies(payload), denied, "{content}");
    }
}

#[test]
fn shell_forms_reach_the_same_decision() {
    let records = records();
    let path = records.path("task.md");
    let calibration = records.path("estimate-calibration.md");
    for (command, denied) in [
        (format!("echo 'model = \"self\"' | tee {path}"), true),
        (format!("echo 'model = \"gpt-6-sol\"' | tee -a {path}"), false),
        (format!("sed -i 's/x/model = auto/' {path}"), true),
        (format!("sed -i 's/x/model = gpt-6-sol/' {path}"), false),
        (format!("perl -pi -e 's/x/model=self/' {path}"), true),
        (format!("bash -c 'echo model=self > {path}'"), true),
        (format!("sh -lc 'echo model=gpt-6-sol > {path}'"), false),
        (format!("echo 'model: pending' >> {calibration}"), true),
        (format!("echo $(date) > {path}"), false),
        (format!("echo model=self && echo done > {path}"), false),
        ("echo 'unterminated".to_owned(), false),
    ] {
        assert_eq!(records.bash(&command), denied, "{command}");
    }
}

fn unrelated_tools_and_shapes_pass() {
    let records = records();
    let path = records.path("task.md");
    assert!(!records.denies(json!({"tool_name": "Read", "tool_input": {"file_path": path}})));
    assert!(!records.denies(json!({"tool_name": "MultiEdit", "tool_input": {"file_path": path, "edits": "x"}})));
}
