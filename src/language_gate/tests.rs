use super::*;
use tempfile::TempDir;

#[derive(Deserialize)]
struct Fixture {
    name: String,
    path: String,
    base: Option<String>,
    head: String,
    allow: bool,
    rule: Option<String>,
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("../fixtures/language-gate/cases.json")).unwrap()
}

fn repository() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]).unwrap();
    git(dir.path(), &["config", "user.name", "Language fixture"]).unwrap();
    git(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    )
    .unwrap();
    dir
}

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "--all"]).unwrap();
    git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            message,
            "--allow-empty",
        ],
    )
    .unwrap();
    String::from_utf8(git(repo, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .into()
}

fn write_list(repo: &Path, patterns: &[(&str, &str)]) {
    let entries: Vec<_> = patterns.iter().map(|(glob, class)| json!({"glob":glob,"class":class,"gap":"fixture native library gap","provenance":"Tom 2026-10-15: permit this fixture"})).collect();
    fs::write(
        repo.join(ALLOWLIST),
        json!({"entries": entries}).to_string(),
    )
    .unwrap();
}

#[test]
fn language_gate_git_revisions_enforce_additions_growth_and_shrinking() {
    for case in fixtures() {
        let dir = repository();
        let repo = dir.path();
        if let Some(content) = &case.base {
            fs::write(repo.join(&case.path), content).unwrap();
        }
        if case.allow {
            write_list(repo, &[(&case.path, "python")]);
        }
        let base = commit(repo, "base");
        fs::write(repo.join(&case.path), &case.head).unwrap();
        let head = commit(repo, "head");
        let result = violations(repo, &base, &head).unwrap();
        match &case.rule {
            Some(rule) => {
                assert_eq!(result.len(), 1, "{}: {result:?}", case.name);
                assert!(
                    result[0].contains(rule)
                        && result[0].contains(&case.path)
                        && result[0].contains(ALTERNATIVE),
                    "{result:?}"
                );
            }
            None => assert!(result.is_empty(), "{}: {result:?}", case.name),
        }
        fs::remove_file(repo.join(&case.path)).unwrap();
        let deleted = commit(repo, "delete");
        assert!(violations(repo, &head, &deleted).unwrap().is_empty());
    }
    let dir = repository();
    let repo = dir.path();
    fs::write(repo.join("old.txt"), "text\n").unwrap();
    fs::write(repo.join("old.py"), "same\n").unwrap();
    let base = commit(repo, "base");
    fs::rename(repo.join("old.txt"), repo.join("renamed.ts")).unwrap();
    fs::rename(repo.join("old.py"), repo.join("renamed.py")).unwrap();
    let head = commit(repo, "renames");
    let result = violations(repo, &base, &head).unwrap();
    assert_eq!(result.len(), 1, "{result:?}");
    assert!(result[0].contains("renamed.ts: R1"), "{result:?}");
    fs::copy(repo.join("renamed.py"), repo.join("copy.py")).unwrap();
    fs::copy(repo.join("renamed.ts"), repo.join("copy.js")).unwrap();
    let copies = commit(repo, "copies");
    let result = violations(repo, &head, &copies).unwrap();
    assert!(result.is_empty(), "{result:?}");
    fs::write(repo.join("runner"), "#!/bin/sh\n").unwrap();
    let before_shebang = commit(repo, "shell runner");
    fs::write(repo.join("runner"), "#!/usr/bin/python3\n").unwrap();
    let after_shebang = commit(repo, "change interpreter without growth");
    assert!(
        violations(repo, &before_shebang, &after_shebang)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn language_gate_guard_decodes_both_providers_and_fails_open() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    fs::create_dir(home.join("code")).unwrap();
    let repository = repository();
    let repo = home.join("code/project");
    fs::rename(repository.path(), &repo).unwrap();
    let decision = |payload: Value| {
        guard_decision(payload.to_string().as_bytes(), home, &repo, false).unwrap()
    };
    for case in fixtures() {
        if case.allow {
            write_list(&repo, &[(&case.path, "python")]);
        } else if repo.join(ALLOWLIST).exists() {
            fs::remove_file(repo.join(ALLOWLIST)).unwrap();
        }
        if let Some(content) = &case.base {
            fs::write(repo.join(&case.path), content).unwrap();
        }
        let denial = decision(
            json!({"tool_name":"Write","cwd":repo,"tool_input":{"file_path":case.path,"content":case.head}}),
        );
        let expected = case.base.is_none() && case.rule.is_some();
        assert_eq!(denial.is_some(), expected, "{}: {denial:?}", case.name);
        if let Some(denial) = denial {
            assert_eq!(denial["hookSpecificOutput"]["hookEventName"], "PreToolUse");
            assert_eq!(denial["hookSpecificOutput"]["permissionDecision"], "deny");
            let reason = denial["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap();
            assert!(reason.contains(ALTERNATIVE) && reason.contains(ALLOWLIST));
        }
    }
    for tool in ["Edit", "MultiEdit", "Write", "NotebookEdit"] {
        assert!(
            decision(json!({"tool_name":tool,"tool_input":{"file_path":"absent.py"}})).is_some(),
            "{tool}"
        );
        assert!(
            decision(json!({"tool_name":tool,"tool_input":{"filePath":"grown.py"}})).is_none(),
            "{tool}"
        );
    }
    for tool_input in [
        json!({"input":"*** Begin Patch\n*** Add File: patch.py\n+print(1)\n*** End Patch"}),
        json!("*** Begin Patch\n*** Add File: runner2\n+#!/usr/bin/python\n*** End Patch"),
    ] {
        assert!(decision(json!({"tool_name":"apply_patch","tool_input":tool_input})).is_some());
    }
    for command in [
        "printf hi > shell.py",
        "echo hi >> 'shell.ts'",
        "tee shell.mts",
        "touch shell.tsx",
        "cp old.txt shell.cts",
        "mv old.txt shell.js",
        "install old.txt shell.mjs",
        "ln -s old.txt shell.cjs",
        "touch harmless.rs && touch shell.py",
        "cp -t . source.py",
    ] {
        assert!(
            decision(json!({"tool_name":"Bash","tool_input":{"command":command}})).is_some(),
            "{command}"
        );
        assert!(decision(json!({"tool_name":"exec_command","tool_input":json!({"cmd":command,"workdir":repo}).to_string()})).is_some(), "{command}");
    }
    for command in [
        "echo 'touch mention.py'",
        "printf '%s' '> mention.py'",
        "touch okay.rs",
        "touch grown.py",
    ] {
        assert!(
            decision(json!({"tool_name":"Bash","tool_input":{"command":command}})).is_none(),
            "{command}"
        );
    }
    assert!(
        decision(json!({"tool_name":"Write","tool_input":{"file_path":home.join("outside.py")}}))
            .is_none()
    );
    for input in [
        b"{".as_slice(),
        b"null",
        b"[]",
        b"",
        &vec![b' '; INPUT_LIMIT as usize + 1],
    ] {
        assert!(
            guard_decision(input, home, &repo, false)
                .unwrap_or(None)
                .is_none()
        );
    }
    let payload = json!({"tool_name":"Write","tool_input":{"file_path":"override.py"}}).to_string();
    assert!(
        guard_decision(payload.as_bytes(), home, &repo, true)
            .unwrap()
            .is_none()
    );
    fs::write(repo.join(ALLOWLIST), "malformed").unwrap();
    assert!(
        guard_decision(payload.as_bytes(), home, &repo, false)
            .unwrap_or(None)
            .is_none()
    );
}

#[test]
fn language_gate_allowlist_expansion_requires_a_commit_trailer() {
    let dir = repository();
    let repo = dir.path();
    write_list(repo, &[("tools/*.py", "python")]);
    let base = commit(repo, "base");
    write_list(repo, &[("**/*.py", "python")]);
    let unapproved = commit(repo, "widen");
    assert!(
        violations(repo, &base, &unapproved)
            .unwrap()
            .iter()
            .any(|line| line.contains("R3"))
    );
    let approved = commit(
        repo,
        "approval\n\nLanguage-allowlist: Tom 2026-10-15 permit Python fixture libraries",
    );
    assert!(violations(repo, &base, &approved).unwrap().is_empty());
    write_list(repo, &[("tools/file?.py", "python")]);
    let narrower = commit(repo, "narrow");
    assert!(violations(repo, &base, &narrower).unwrap().is_empty());
    let broad = Allowlist::parse(br#"{"entries":[{"glob":"**/*.py","class":"python","gap":"library","provenance":"Tom 2026-10-15 yes"}]}"#).unwrap();
    for path in ["top.py", "tools/a.py", "a/b/c.py"] {
        assert!(broad.allows(path, Class::Python), "{path}");
    }
    for (pattern, path, expected) in [
        ("*.py", "a/b.py", false),
        ("a/?.py", "a/b.py", true),
        ("a/?.py", "a/bb.py", false),
        ("**/foo.py", "xfoo.py", false),
        ("a/**/b.py", "a/b.py", true),
    ] {
        assert_eq!(glob_matches(pattern, path), expected, "{pattern} {path}");
    }
    for invalid in [
        br#"{"entries":[{"glob":"*.py","class":"python","gap":"","provenance":"Tom"}]}"#.as_slice(),
        br#"{"entries":[{"glob":"*.py","class":"python","gap":"library","provenance":" "}]}"#,
    ] {
        assert!(Allowlist::parse(invalid).is_err());
    }
}
