use super::*;

fn pair(model: Option<&str>, effort: Option<&str>) -> (Option<String>, Option<String>) {
    (model.map(str::to_owned), effort.map(str::to_owned))
}

#[test]
fn the_role_files_frontmatter_is_read_and_a_body_line_is_not() {
    assert_eq!(fields("---\nname: coordinator\nmodel: fable\neffort: high\n---\nBody.\n"), pair(Some("fable"), Some("high")));
    assert_eq!(fields("---\nname: coordinator\nmodel:  fable \n---\nBody.\n"), pair(Some("fable"), None), "a missing effort stays absent");
    assert_eq!(fields("No frontmatter at all.\nmodel: fable\n"), pair(None, None), "only the frontmatter counts");
    assert_eq!(
        fields(include_str!("../../../harness/claude/agents/coordinator.md")),
        pair(Some("fable"), Some("high")),
        "north#1048 moves the coordinator to Fable high, and this is the file both Claude Code and this launch read it from"
    );
}
