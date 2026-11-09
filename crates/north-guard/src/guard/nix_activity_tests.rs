#![cfg(test)]

use super::nix_activity::decide;
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn verdict(cwd: &std::path::Path, command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": cwd, "tool_input": {"command": command}});
    decide(&HookInput::parse(payload.to_string().as_bytes()).unwrap(), cwd)
}

#[test]
fn an_ad_hoc_nixpkgs_shell_is_refused_only_where_the_command_runs_beside_a_dev_shell_flake() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("flake.nix"), "{ devShells = {}; }").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let scratch = scratch.path().display();
    assert!(matches!(verdict(project.path(), "nix shell nixpkgs#rustc -c rustc a.rs"), Verdict::Deny(_)));
    assert!(matches!(verdict(project.path(), &format!("cd {scratch} && (rustc a.rs || nix shell nixpkgs#rustc -c rustc a.rs) | tail -5")), Verdict::Allow));
    assert!(matches!(verdict(project.path(), &format!("cd {scratch} && cd {} && nix shell nixpkgs#rustc -c rustc a.rs", project.path().display())), Verdict::Deny(_)));
}
