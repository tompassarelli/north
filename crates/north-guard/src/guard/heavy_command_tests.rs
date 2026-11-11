#![cfg(test)]

use super::heavy_command::judge;
use crate::hook_input::{HookInput, Verdict};
use serde_json::json;

fn verdict(command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": "/", "tool_input": {"command": command}});
    judge(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

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

fn verdict_in(cwd: &std::path::Path, command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "cwd": cwd, "tool_input": {"command": command}});
    judge(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

#[test]
fn bun_test_with_any_positional_or_module_script_is_not_the_full_suite() {
    for command in [
        "cd lib/automation-chrome && timeout 25 bun test service.test.mjs 2>&1 | tail -5",
        "bun test actions.test.cjs",
        "bun test a.test.mts",
        "bun test auth",
    ] {
        assert!(!matches!(verdict(command), Verdict::Deny(_)), "{command} was refused");
    }
    for command in ["bun test", "bun test 2>&1 | tail -5", "bun run test", "bun test -t name"] {
        assert!(matches!(verdict(command), Verdict::Deny(_)), "{command} was not refused");
    }
}

#[test]
fn a_narrowing_variable_set_inside_a_background_group_permits_a_farm_only_run() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir(dir.path().join(".agents")).unwrap();
    std::fs::write(
        dir.path().join(".agents/farm-only.toml"),
        "[[command]]\npattern = [\"bun\", \"scripts/lua-tests.ts\"]\nnarrowing_env = [\"GAME_TESTS\", \"GAME_MODULES\"]\nfarm = \"farm\"\nwhy = \"full suite\"\n",
    )
    .unwrap();
    for command in [
        "(GAME_MODULES=$(cat m.txt) bun scripts/lua-tests.ts > out.log 2>&1) &",
        "GAME_TESTS=$(cat t.txt) bun scripts/lua-tests.ts > out.log",
        "( GAME_MODULES=a bun scripts/lua-tests.ts ) &",
        "x=$(GAME_TESTS=a bun scripts/lua-tests.ts)",
    ] {
        assert!(!matches!(verdict_in(dir.path(), command), Verdict::Deny(_)), "{command} was refused");
    }
    assert!(matches!(verdict_in(dir.path(), "(bun scripts/lua-tests.ts > out.log) &"), Verdict::Deny(_)));
    assert!(matches!(verdict_in(dir.path(), "bun scripts/lua-tests.ts"), Verdict::Deny(_)));
}

#[test]
fn a_full_bun_test_of_a_small_suite_runs_under_a_bound_instead_of_being_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.test.ts"), "").unwrap();
    for args in [&["init", "-q"][..], &["add", "a.test.ts"]] {
        assert!(std::process::Command::new("git").arg("-C").arg(dir.path()).args(args).status().unwrap().success());
    }
    let Verdict::Rewrite(updated, _) = verdict_in(dir.path(), "echo x > a.ts && bun test 2>&1 | tail -5") else {
        panic!("the full bun test of a one-file suite was not bounded")
    };
    assert!(updated["command"].as_str().unwrap().starts_with("timeout -k 5 15 bash -c 'echo x > a.ts && bun test 2>&1 | tail -5';"));
}

#[test]
fn a_full_bun_test_of_a_sixty_one_file_suite_is_bounded_while_one_outside_a_checkout_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git").arg("-C").arg(dir.path()).args(args).status().unwrap().success());
    };
    git(&["init", "-q"]);
    for index in 0..61 {
        std::fs::write(dir.path().join(format!("case{index}.test.ts")), "").unwrap();
    }
    git(&["add", "."]);
    assert!(matches!(verdict_in(dir.path(), "bun test"), Verdict::Rewrite(..)));
    assert!(matches!(verdict("bun test"), Verdict::Deny(_)));
}
