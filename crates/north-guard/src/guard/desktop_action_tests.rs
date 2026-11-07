use super::dangerous;
use std::collections::BTreeMap;

fn refused(command: &str) -> bool {
    dangerous(command, &BTreeMap::new())
}

#[test]
fn a_backtick_inside_single_quotes_is_text_not_a_substitution() {
    assert!(!refused("grep -v -e '^- Never use `ydotool`' -e '^- Never run `niri msg action focus-window`' notes.md"));
    assert!(!refused("echo 'run $(ydotool key 1)'"));
    assert!(refused("echo \"`ydotool key 1`\""));
    assert!(refused("echo $(ydotool key 1)"));
    assert!(refused("bash -c 'ydotool key 1'"));
    assert!(refused("bash -c 'echo `ydotool key 1`'"));
    assert!(refused("niri msg action close-window --id 161"));
}
