#![cfg(test)]

use super::blind_stage;
use crate::hook_input::{HookInput, Verdict};
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_refused_git_commit_am_prints_the_command_that_stages_and_commits_the_changed_files() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("lane");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "north"]);
    git(&repo, &["config", "user.email", "north@example.invalid"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("kept.rs"), "one\n").unwrap();
    std::fs::write(repo.join("with space.rs"), "one\n").unwrap();
    git(&repo, &["add", "kept.rs", "with space.rs"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    std::fs::write(repo.join("kept.rs"), "two\n").unwrap();
    std::fs::write(repo.join("with space.rs"), "two\n").unwrap();
    std::fs::write(repo.join("stray.log"), "noise\n").unwrap();

    let command = format!("cd {} && git commit -am 'Fix the tourney preset'", repo.display());
    let call = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": scratch.path(), "tool_input": {"command": command}});
    let Verdict::Deny(refusal) = blind_stage::decide(&HookInput::parse(call.to_string().as_bytes()).unwrap()) else {
        panic!("git commit -am behind a cd is refused");
    };

    let printed = refusal.split('`').nth(1).unwrap();
    assert_eq!(printed, format!("cd {} && git add -- kept.rs 'with space.rs' && git commit -m 'Fix the tourney preset'", repo.display()));
    assert!(refusal.contains("stray.log"), "{refusal}");
    let retry = Command::new("sh").arg("-c").arg(printed).current_dir(scratch.path()).output().unwrap();
    assert!(retry.status.success(), "{}", String::from_utf8_lossy(&retry.stderr));
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? stray.log\n");
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "Fix the tourney preset\n");
}
