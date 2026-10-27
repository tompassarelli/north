use super::*;
use tempfile::TempDir;
use std::process::Command;

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
    commit_on(repo, message, "2026-10-15T12:00:00+00:00")
}

fn commit_on(repo: &Path, message: &str, date: &str) -> String {
    git(repo, &["add", "--all"]).unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_AUTHOR_DATE", "2026-10-15T12:00:00+00:00")
        .env("GIT_COMMITTER_DATE", date)
        .args([
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            message,
            "--allow-empty",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(git(repo, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .into()
}

fn write_list(repo: &Path, patterns: &[(&str, &str)]) {
    let entries: Vec<_> = patterns.iter().map(|(glob, class)| json!({"glob":glob,"class":class,"gap":"fixture native library gap","provenance":"Tom 2026-10-15: permit this fixture","port_owner":"fixture-port-owner","port_target":"src/ported.rs","commitment":"Rust port by 2026-10-16 18:00","created":"2026-10-15"})).collect();
    fs::write(
        repo.join(ALLOWLIST),
        json!({"ratchet":["script"],"entries": entries}).to_string(),
    )
    .unwrap();
}

#[test]
fn language_gate_git_revisions_enforce_additions_growth_and_shrinking() {
    for case in fixtures() {
        let dir = repository();
        let repo = dir.path();
        write_list(repo, &[]);
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
    write_list(repo, &[]);
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
    let mut result = violations(repo, &head, &copies).unwrap();
    result.sort();
    assert_eq!(result.len(), 2, "{result:?}");
    assert!(result[0].starts_with("copy.js: R1") && result[1].starts_with("copy.py: R1"), "{result:?}");
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
        } else {
            write_list(&repo, &[]);
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
    write_list(repo, &[("tools/*.sh", "shell")]);
    let base = commit(repo, "base");
    write_list(repo, &[("**/*.sh", "shell")]);
    let unapproved = commit(repo, "widen");
    assert!(
        violations(repo, &base, &unapproved)
            .unwrap()
            .iter()
            .any(|line| line.contains("R3"))
    );
    let approved = commit(
        repo,
        "approval\n\nLanguage-allowlist: Tom 2026-10-15 permit shell fixture libraries",
    );
    assert!(violations(repo, &base, &approved).unwrap().is_empty());
    write_list(repo, &[("tools/file?.sh", "shell")]);
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

#[test]
fn language_gate_reference_cases_cap_shell_glue_at_80_code_lines() {
    let cases: Vec<Fixture> = serde_json::from_str(include_str!(
        "../fixtures/language-gate/shell-ratchet-reference.json"
    ))
    .unwrap();
    for case in cases {
        let dir = repository();
        let repo = dir.path();
        if let Some(content) = &case.base {
            fs::write(repo.join(&case.path), content).unwrap();
        }
        if case.allow {
            let class = match classify(Path::new(&case.path), case.head.as_bytes()).unwrap() {
                Class::Shell => "shell",
                Class::Python => "python",
                Class::Script => "script",
            };
            write_list(repo, &[(&case.path, class)]);
        }
        let base = commit(repo, "base");
        fs::write(repo.join(&case.path), &case.head).unwrap();
        let head = commit(repo, "head");
        let failures = violations(repo, &base, &head).unwrap();
        match &case.rule {
            Some(rule) => {
                assert_eq!(failures.len(), 1, "{}: {failures:?}", case.name);
                assert!(failures[0].contains(rule), "{}: {failures:?}", case.name);
            }
            None => assert!(failures.is_empty(), "{}: {failures:?}", case.name),
        }
    }
    for interpreter in ["sh", "bash", "zsh", "dash"] {
        for shebang in [format!("#!/bin/{interpreter}"), format!("#!/usr/bin/env -S {interpreter} -e")] {
            let content = format!("{shebang}\n{}", "echo glue\n".repeat(81));
            assert_eq!(classify(Path::new("tool"), content.as_bytes()), Some(Class::Shell));
            assert_eq!(shell_code_lines(content.as_bytes()), 81);
        }
    }
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let repo = home.join("code/project");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]).unwrap();
    write_list(&repo, &[]);
    for path in ["tool.sh", "tool.bash", "tool"] {
        let payload = json!({"tool_name":"Write","tool_input":{"file_path":path,"content":format!("#!/bin/sh\n{}", "echo glue\n".repeat(81))}});
        assert!(guard_decision(payload.to_string().as_bytes(), home, &repo, false).unwrap().is_none(), "{path}");
    }
}

#[test]
fn language_gate_script_additions_growth_and_guard_require_repo_opt_in() {
    for extension in ["ts", "tsx", "mts", "cts", "js", "mjs", "cjs"] {
        for policy in [None, Some(false), Some(true)] {
            let temporary = tempfile::tempdir().unwrap();
            let home = temporary.path();
            let repo = home.join("code/project");
            fs::create_dir_all(repo.parent().unwrap()).unwrap();
            let dir = repository();
            fs::rename(dir.path(), &repo).unwrap();
            if let Some(ratchet) = policy {
                fs::write(repo.join(ALLOWLIST), json!({"ratchet": if ratchet {vec!["script"]} else {vec![]}, "entries":[]}).to_string()).unwrap();
            }
            let path = format!("tool.{extension}");
            let base = commit(&repo, "base");
            let payload = json!({"tool_name":"Write","tool_input":{"file_path":path,"content":"one();\n"}});
            assert_eq!(guard_decision(payload.to_string().as_bytes(), home, &repo, false).unwrap().is_some(), policy == Some(true), "{extension} {policy:?}");
            fs::write(repo.join(&path), "one();\n").unwrap();
            let added = commit(&repo, "add");
            assert_eq!(!violations(&repo, &base, &added).unwrap().is_empty(), policy == Some(true), "{extension} {policy:?}");
            fs::write(repo.join(&path), "one();\ntwo();\n").unwrap();
            let grown = commit(&repo, "grow");
            assert_eq!(!violations(&repo, &added, &grown).unwrap().is_empty(), policy == Some(true), "{extension} {policy:?}");
            fs::write(repo.join(&path), "other();\nstill();\n").unwrap();
            let same_size = commit(&repo, "edit");
            assert!(violations(&repo, &grown, &same_size).unwrap().is_empty());
        }
    }
}

#[test]
fn language_gate_removing_script_ratchet_requires_approval() {
    for replacement in [Some(json!({"ratchet":[],"entries":[]})), Some(json!({"entries":[]})), None] {
        let dir = repository();
        let repo = dir.path();
        let empty = commit(repo, "empty");
        write_list(repo, &[]);
        let base = commit(repo, "opt in");
        assert!(violations(repo, &empty, &base).unwrap().is_empty());
        match replacement {
            Some(value) => fs::write(repo.join(ALLOWLIST), value.to_string()).unwrap(),
            None => fs::remove_file(repo.join(ALLOWLIST)).unwrap(),
        }
        let removed = commit(repo, "remove ratchet");
        let failures = violations(repo, &base, &removed).unwrap();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("R3"), "{failures:?}");
        let approved = commit(repo, "approval\n\nLanguage-allowlist: Tom 2026-10-15 allow script ratchet removal");
        assert!(violations(repo, &base, &approved).unwrap().is_empty());
    }
}

#[test]
fn language_gate_first_push_from_empty_tree_checks_every_commit() {
    let dir = repository();
    let repo = dir.path();
    write_list(repo, &[]);
    fs::write(repo.join("main.rs"), "fn main() {}\n").unwrap();
    let rust = commit(repo, "rust");
    assert!(violations(repo, EMPTY_TREE, &rust).unwrap().is_empty());
    fs::write(repo.join("tool.ts"), "print(1)\n").unwrap();
    let script = commit(repo, "script");
    let failures = violations(repo, EMPTY_TREE, &script).unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("R1"), "{failures:?}");
    write_list(repo, &[("tool.ts", "script")]);
    let listed = commit(repo, "list");
    let failures = violations(repo, EMPTY_TREE, &listed).unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("R3"), "{failures:?}");
    let approved = commit(repo, "approval\n\nLanguage-allowlist: Tom 2026-10-15 permit Script fixture library");
    assert!(violations(repo, EMPTY_TREE, &approved).unwrap().is_empty());
}

#[test]
fn language_gate_copies_count_as_new_files() {
    let dir = repository();
    let repo = dir.path();
    fs::write(repo.join("old.py"), "a = 1\nb = 2\nc = 3\n").unwrap();
    fs::write(repo.join("big.sh"), format!("#!/bin/sh\n{}", "echo glue\n".repeat(100))).unwrap();
    let base = commit(repo, "base");
    fs::copy(repo.join("old.py"), repo.join("copy.py")).unwrap();
    fs::copy(repo.join("big.sh"), repo.join("copy.sh")).unwrap();
    let head = commit(repo, "copies");
    let mut failures = violations(repo, &base, &head).unwrap();
    failures.sort();
    assert_eq!(failures.len(), 2, "{failures:?}");
    assert!(failures[0].starts_with("copy.py: R1"), "{failures:?}");
    assert!(failures[1].starts_with("copy.sh: R2"), "{failures:?}");
}

#[derive(Deserialize)]
struct PythonPortFixtures {
    entry: Value,
    cases: Vec<PythonPortCase>,
}

#[derive(Deserialize)]
struct PythonPortCase {
    group: String,
    name: String,
    date: String,
    #[serde(default)]
    changes: serde_json::Map<String, Value>,
    remove: Option<String>,
    #[serde(default)]
    landed: bool,
    #[serde(default)]
    growth: bool,
    #[serde(default)]
    new_path: bool,
    rule: Option<String>,
    field: Option<String>,
}

fn python_port_cases(group: &str) {
    let fixtures: PythonPortFixtures = serde_json::from_str(include_str!(
        "../fixtures/language-gate/python-port-owner.json"
    ))
    .unwrap();
    let mut exercised = false;
    for case in fixtures.cases.iter().filter(|case| case.group == group) {
        exercised = true;
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("code/project");
        fs::create_dir_all(repo.parent().unwrap()).unwrap();
        let dir = repository();
        fs::rename(dir.path(), &repo).unwrap();
        write_list(&repo, &[]);
        if !case.new_path {
            fs::write(repo.join("tool.py"), "print(1)\n").unwrap();
        }
        let base = commit(&repo, "base");
        let mut entry = fixtures.entry.clone();
        let fields = entry.as_object_mut().unwrap();
        fields.extend(case.changes.clone());
        if let Some(field) = &case.remove {
            fields.remove(field);
        }
        fs::write(
            repo.join(ALLOWLIST),
            json!({"ratchet":["script"],"entries":[entry]}).to_string(),
        )
        .unwrap();
        if case.landed {
            fs::create_dir(repo.join("src")).unwrap();
            fs::write(repo.join("src/ported.rs"), "fn main() {}\n").unwrap();
        }
        if case.new_path || case.growth {
            fs::write(repo.join("tool.py"), "print(1)\nprint(2)\n").unwrap();
        }
        let head = commit_on(&repo, "head without approval trailer", &case.date);
        let failures = violations(&repo, &base, &head).unwrap();
        if let Some(rule) = &case.rule {
            assert_eq!(failures.len(), 1, "{}: {failures:?}", case.name);
            assert!(
                failures[0].contains(rule) && failures[0].contains("tool.py"),
                "{}: {failures:?}",
                case.name
            );
            if let Some(field) = &case.field {
                assert!(failures[0].contains(field), "{}: {failures:?}", case.name);
            }
        } else {
            assert!(failures.is_empty(), "{}: {failures:?}", case.name);
        }
        if case.rule.as_deref() == Some("R4") {
            write_list(&repo, &[]);
            let removed = commit_on(&repo, "remove expired entry", &case.date);
            assert!(
                violations(&repo, &base, &removed).unwrap().is_empty(),
                "{}",
                case.name
            );
        }
        if case.new_path {
            fs::remove_file(repo.join("tool.py")).unwrap();
            let payload = json!({"tool_name":"Write","tool_input":{"file_path":"tool.py","content":"print(1)\n"}});
            assert!(
                guard_decision(payload.to_string().as_bytes(), home.path(), &repo, false)
                    .unwrap()
                    .is_some()
            );
        }
    }
    assert!(exercised, "no Python port fixtures for {group}");
}

#[test]
fn language_gate_python_entries_require_a_named_rust_port_and_eta() {
    python_port_cases("metadata");
}

#[test]
fn language_gate_python_entries_expire_when_the_rust_port_lands() {
    python_port_cases("landed");
}

#[test]
fn language_gate_python_entries_expire_after_seven_commit_date_days() {
    python_port_cases("age");
}

#[test]
fn language_gate_valid_python_entries_permit_existing_file_growth() {
    python_port_cases("growth");
}

#[test]
fn language_gate_valid_python_entries_never_permit_new_files() {
    python_port_cases("new");
}

#[test]
fn language_gate_standing_python_approval_preserves_shell_trailers() {
    python_port_cases("approval");
    let dir = repository();
    let repo = dir.path();
    write_list(repo, &[]);
    let base = commit(repo, "base");
    write_list(repo, &[("tool.sh", "shell")]);
    let head = commit(repo, "shell entry without approval");
    let failures = violations(repo, &base, &head).unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("R3"), "{failures:?}");
    let approved = commit(
        repo,
        "approval\n\nLanguage-allowlist: Tom 2026-10-15 permit shell fixture",
    );
    assert!(violations(repo, &base, &approved).unwrap().is_empty());
}
