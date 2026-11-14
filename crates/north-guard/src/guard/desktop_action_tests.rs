use super::{closable, dangerous};
use serde_json::json;
use std::collections::BTreeMap;

fn refused(command: &str) -> bool {
    dangerous(command, &BTreeMap::new())
}

#[test]
fn backtick_text_is_not_a_substitution_and_only_a_lone_own_window_closes() {
    assert!(!refused("grep -v -e '^- Never use `ydotool`' -e '^- Never run `niri msg action focus-window`' notes.md"));
    assert!(!refused("echo 'run $(ydotool key 1)'"));
    assert!(refused("echo \"`ydotool key 1`\""));
    assert!(refused("echo $(ydotool key 1)"));
    assert!(refused("bash -c 'ydotool key 1'"));
    assert!(refused("bash -c 'echo `ydotool key 1`'"));
    assert!(refused("niri msg action close-window --id 161"));
    let windows = json!([
        {"id": 1, "pid": 300, "app_id": "foot", "is_focused": true},
        {"id": 2, "pid": 400, "app_id": "ghostty", "is_focused": false},
        {"id": 3, "pid": 400, "app_id": "ghostty", "is_focused": false},
        {"id": 4, "pid": 500, "app_id": "quiet.spawn", "is_focused": false},
        {"id": 5, "pid": 600, "app_id": "quiet.spawn", "is_focused": true}
    ]);
    assert!(closable(&windows, 1, &[900, 300]));
    assert!(!closable(&windows, 1, &[900]));
    assert!(!closable(&windows, 2, &[400]));
    assert!(!closable(&windows, 3, &[400]));
    assert!(closable(&windows, 4, &[]));
    assert!(!closable(&windows, 5, &[]));
    assert!(!closable(&windows, 9, &[300]));
}
