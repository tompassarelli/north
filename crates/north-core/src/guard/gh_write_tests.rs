use super::*;

fn only(command: &str, cwd: Option<&str>) -> GhWrite {
    let mut writes = writes(command, cwd);
    assert_eq!(writes.len(), 1, "{command}: {writes:?}");
    writes.remove(0)
}

#[test]
fn reads_inline_text_files_fields_and_heredocs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("body.md"), "from a file").unwrap();
    let cwd = dir.path().to_str();
    assert_eq!(only("gh issue comment 5 --body inline", cwd).bodies, ["inline"]);
    assert_eq!(only("gh issue comment 5 --body-file body.md", cwd).bodies, ["from a file"]);
    assert_eq!(only("gh issue comment 5 -F body.md", cwd).bodies, ["from a file"]);
    assert_eq!(only("gh api repos/o/r/issues/5/comments -F body=@body.md", cwd).bodies, ["from a file"]);
    assert_eq!(only("gh api repos/o/r/issues/5/comments -f body=literal", cwd).bodies, ["literal"]);
    assert_eq!(only("gh issue comment 5 --body \"$(cat body.md)\"", cwd).bodies, ["from a file"]);
    assert_eq!(
        only("gh issue comment 5 --body-file - <<'EOF'\nfrom stdin\nEOF", cwd).bodies,
        ["from stdin\n"]
    );
}

#[test]
fn names_the_repository_and_ignores_reads_and_other_commands() {
    assert_eq!(only("gh -R o/r issue comment 5 --body x", None).repo.as_deref(), Some("o/r"));
    assert_eq!(only("gh issue comment 5 --repo=o/r --body x", None).repo.as_deref(), Some("o/r"));
    assert_eq!(only("gh api repos/o/r/issues/5/comments -f body=x", None).repo.as_deref(), Some("o/r"));
    for command in ["gh issue view 5", "gh pr list", "echo gh issue comment 5 --body x", "git commit -m x"] {
        assert!(writes(command, None).is_empty(), "{command}");
    }
}

#[test]
fn finds_the_target_repository_past_method_flags_urls_and_environment() {
    let repo = |command: &str| only(command, None).repo;
    assert_eq!(repo("gh api -X POST repos/o/r/issues/5/comments -f body=x").as_deref(), Some("o/r"));
    assert_eq!(repo("gh api --method PATCH -H 'Accept: x' /repos/o/r/issues/5 -f body=x").as_deref(), Some("o/r"));
    assert_eq!(repo("gh pr comment https://github.com/o/r/pull/1 --body x").as_deref(), Some("o/r"));
    assert_eq!(repo("GH_REPO=o/r gh issue comment 5 --body x").as_deref(), Some("o/r"));
    assert_eq!(repo("gh issue comment 5 --body x"), None);
}

#[test]
fn a_trailing_flag_without_a_value_keeps_the_write_and_a_boolean_m_is_not_text() {
    assert_eq!(only("gh pr merge 5 --body secret -m", None).bodies, ["secret"]);
    assert_eq!(only("gh pr merge 5 -m --body secret", None).bodies, ["secret"]);
    assert_eq!(only("gh issue comment 5 --body ok --body", None).bodies, ["ok"]);
}

#[test]
fn reads_stdin_files_embedded_substitutions_and_gist_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.md"), "from a file").unwrap();
    let cwd = dir.path().to_str();
    assert_eq!(only("gh issue comment 5 -F - < note.md", cwd).bodies, ["from a file", "note.md"]);
    assert_eq!(only("gh issue comment 5 --body \"Summary: $(cat note.md)\"", cwd).bodies, ["from a file", "Summary: "]);
    assert_eq!(only("gh gist create note.md --desc x", cwd).bodies, ["x", "from a file"]);
    assert_eq!(only("gh label create bug --description 'a label'", cwd).bodies, ["a label"]);
}

#[test]
fn marks_text_it_cannot_see_opaque() {
    assert!(only("printf x | gh issue comment 5 --body-file -", None).opaque);
    assert!(only("gh issue comment 5 --body \"$summary\"", None).opaque);
    assert!(!only("gh issue comment 5 --body 'costs $5'", None).opaque);
    assert!(!only("gh issue comment 5 --body plain", None).opaque);
}

#[test]
fn finds_a_gh_write_inside_backticks() {
    assert_eq!(only("x=`gh issue comment 5 --body inline`", None).bodies, ["inline"]);
}

#[test]
fn reads_close_comments_and_short_descriptions() {
    assert_eq!(only("gh issue close 5 -c 'closing note'", None).bodies, ["closing note"]);
    assert_eq!(only("gh repo edit -d 'a repo'", None).bodies, ["a repo"]);
    assert_eq!(only("gh label create bug -d 'a label'", None).bodies, ["a label"]);
    assert!(only("gh pr create -d --title t", None).bodies == ["t"]);
}

#[test]
fn follows_cd_and_ignores_placeholder_repositories() {
    assert_eq!(only("cd /tmp/x && gh issue comment 5 --body y", Some("/home")).dir.as_deref(), Some("/tmp/x"));
    assert_eq!(only("cd x && gh issue comment 5 --body y", Some("/home")).dir.as_deref(), Some("/home/x"));
    assert_eq!(only("gh issue comment 5 --body y", Some("/home")).dir.as_deref(), Some("/home"));
    assert_eq!(only("gh api repos/{owner}/{repo}/issues/5/comments -f body=x", None).repo, None);
}

#[test]
fn reads_files_piped_in_and_marks_backticks_opaque() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.md"), "from a pipe").unwrap();
    let cwd = dir.path().to_str();
    assert!(only("cat note.md | gh issue comment 5 -F -", cwd).bodies.contains(&"from a pipe".to_owned()));
    assert!(only("gh issue comment 5 --body \"see `cat note.md`\"", cwd).opaque);
}

#[test]
fn a_review_comment_switch_does_not_swallow_the_body_and_stdin_gists_are_opaque() {
    assert_eq!(only("gh pr review 5 --comment --body secret", None).bodies, ["secret"]);
    assert_eq!(only("gh repo create name -c -d 'a repo'", None).bodies, ["a repo"]);
    assert!(only("printf x | gh gist create -", None).opaque);
}

#[test]
fn marks_targets_that_are_not_a_checkable_repository_public() {
    assert!(only("gh gist create note.md", None).public);
    assert!(only("gh repo create name", None).public);
    assert!(only("gh api graphql -f query=x", None).public);
    assert!(only("gh api user/repos -f name=x", None).public);
    assert!(!only("gh api repos/{owner}/{repo}/issues/5/comments -f body=x", None).public);
    assert!(!only("gh api repos/o/r/issues/5/comments -f body=x", None).public);
    assert!(!only("gh issue comment 5 --body x", None).public);
    assert_eq!(only("gh repo edit o/r -d x", None).repo.as_deref(), Some("o/r"));
}
