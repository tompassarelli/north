use super::*;

fn pair(model: Option<&str>, effort: Option<&str>) -> (Option<String>, Option<String>) {
    (model.map(str::to_owned), effort.map(str::to_owned))
}

#[test]
fn the_launch_names_the_model_and_effort_the_role_file_gives_the_proxy() {
    let words = launch_words("/home/tom/code/north/main", "opening", pair(Some("fable"), Some("high")));
    assert_eq!(
        words,
        [
            "--provider", "claude", "--role", "proxy", "--parent", "none", "--cwd", "/home/tom/code/north/main", "--model", "fable", "--tier-reason", TIER_REASON, "--effort", "high", "--take-over", "opening"
        ],
        "the record north fleet show proxy prints takes its model from the launch request, so the launch names the pair"
    );
}

#[test]
fn a_role_file_that_names_no_model_or_effort_launches_as_it_did_before() {
    let words = launch_words("/w/main", "opening", pair(None, None));
    assert_eq!(words, ["--provider", "claude", "--role", "proxy", "--parent", "none", "--cwd", "/w/main", "--take-over", "opening"]);
}

#[test]
fn the_role_files_frontmatter_is_read_and_a_body_line_is_not() {
    assert_eq!(fields("---\nname: proxy\nmodel: fable\neffort: high\n---\nBody.\n"), pair(Some("fable"), Some("high")));
    assert_eq!(fields("---\nname: proxy\nmodel:  fable \n---\nBody.\n"), pair(Some("fable"), None), "a missing effort stays absent");
    assert_eq!(fields("No frontmatter at all.\nmodel: fable\n"), pair(None, None), "only the frontmatter counts");
}

#[test]
fn the_shipped_role_file_names_the_fable_high_the_box_moves_the_proxy_to() {
    assert_eq!(
        fields(include_str!("../../../harness/claude/agents/proxy.md")),
        pair(Some("fable"), Some("high")),
        "north#1048 moves the proxy to Fable high, and this is the file both Claude Code and this launch read it from"
    );
}
