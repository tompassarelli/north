use super::*;

fn only(command: &str, cwd: Option<&str>) -> GhWrite {
    let mut writes = writes(command, cwd);
    assert_eq!(writes.len(), 1, "{command}: {writes:?}");
    writes.remove(0)
}

#[test]
fn reads_a_body_file_named_by_a_variable_the_command_assigns_north_1041() {
    let dir = tempfile::tempdir().unwrap();
    let body = "Category: tooling\n\n## Done when\n\n- [ ] the guard reads this file\n";
    std::fs::write(dir.path().join("body.md"), body).unwrap();
    let path = dir.path().display();
    let plain = only(&format!("f={path}/body.md && gh issue create -R tompassarelli/north --body-file $f"), None);
    assert_eq!(plain.bodies.join("\n"), body);
    assert!(!plain.unread);
    let braced = only(&format!("F={path} && gh issue create -R tompassarelli/north --body \"$(cat ${{F}}/body.md)\""), None);
    assert_eq!(braced.bodies.join("\n"), body);
    let same_segment = only(&format!("F={path}/body.md gh issue create -R tompassarelli/north --body-file \"$F\""), None);
    assert_eq!(same_segment.bodies.join("\n"), body);
    let text = only("F=/tmp/gone gh issue create -R tompassarelli/north --body 'costs $F'", None);
    assert_eq!(text.bodies.join("\n"), "costs $F");
}
