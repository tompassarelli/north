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

