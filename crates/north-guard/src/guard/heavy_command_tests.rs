#![cfg(test)]

use super::heavy_command::evaluate;
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn verdict(command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": "/", "tool_input": {"command": command}});
    evaluate(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

// KEEP(bug): an offloaded `sed -i f && git add f && cargo clippy` edited and staged f on gex45 while the local worktree stayed unchanged; north#674
#[test]
fn a_build_beside_a_file_or_index_write_is_refused_and_a_read_only_one_still_offloads() {
    for command in [
        "sed -i s/a/b/ f && cargo check",
        "cd crates/x && git add f && cargo clippy -p x --tests",
        "cargo fmt && cargo test",
        "cargo test 2>&1 > /tmp/log",
        "bash -c 'perl -pi -e s/a/b/ f; cargo build'",
    ] {
        let Verdict::Deny(reason) = verdict(command) else {
            panic!("{command} was not refused")
        };
        assert!(reason.contains("would not reach this worktree"), "{reason}");
    }
    for command in [
        "cd crates/x && cargo check 2>&1 | tail -20",
        "git diff --stat; cargo test -p x > /dev/null",
    ] {
        assert!(
            matches!(verdict(command), Verdict::Rewrite(..)),
            "{command} was not offloaded"
        );
    }
}
