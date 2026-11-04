use super::codex_conduct::{Ctx, decide};
use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    ctx: Ctx,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let ctx = Ctx { state_root: root.join("state"), code_root: root.join("code"), now: 1_000_000.0 };
    World { _dir: dir, root, ctx }
}

fn git(dir: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(dir).args(arguments).output().unwrap().status;
    assert!(status.success(), "git {arguments:?}");
}

fn repo(path: &Path, file: &str) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q"]);
    std::fs::write(path.join(file), "a\n").unwrap();
    git(path, &["add", file]);
    git(path, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
}

fn run(world: &World, payload: Value) -> Verdict {
    decide(&world.ctx, &HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

fn bash(world: &World, kind: &str, command: &str, cwd: &Path, session: &str) -> Verdict {
    bash_response(world, kind, command, cwd, session, "Exit code: 0\nOutput:\nok")
}

fn bash_response(world: &World, kind: &str, command: &str, cwd: &Path, session: &str, response: &str) -> Verdict {
    run(world, json!({"hook_event_name": kind, "session_id": session, "cwd": cwd, "tool_name": "Bash", "tool_input": {"command": command}, "tool_response": response}))
}

fn denied(verdict: &Verdict) -> bool {
    matches!(verdict, Verdict::Deny(_))
}

fn stop(world: &World, message: &str, session: &str) -> bool {
    denied(&run(world, json!({"hook_event_name": "Stop", "session_id": session, "stop_hook_active": false, "last_assistant_message": message})))
}

#[test]
fn a_passing_check_is_not_rerun_on_unchanged_code_and_reruns_after_a_change_or_failure() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    assert_eq!(run(&w, json!({"hook_event_name": "PreToolUse", "session_id": "plain-codex", "cwd": game, "tool_name": "apply_patch", "tool_input": {}})), Verdict::Allow);
    assert!(!denied(&bash(&w, "PreToolUse", "bun test", &game, "boundary")));
    bash(&w, "PostToolUse", "bun test", &game, "boundary");
    assert!(denied(&bash(&w, "PreToolUse", "bun test", &game, "boundary")));
    std::fs::write(game.join("arc.ts"), "b\n").unwrap();
    assert!(!denied(&bash(&w, "PreToolUse", "bun test", &game, "boundary")));
    bash_response(&w, "PostToolUse", "bun test", &game, "boundary", "Exit code: 1\nOutput:\nfailed");
    assert!(!denied(&bash(&w, "PreToolUse", "bun test", &game, "boundary")));
}

#[test]
fn a_farm_run_passed_on_a_commit_is_refused_to_any_other_session() {
    let w = world();
    let farm = w.root.join("code/wisp/main");
    repo(&farm, "a.ts");
    assert!(!denied(&bash(&w, "PreToolUse", r#"bun wisp farm balance "wren expert 120" --wait"#, &farm, "first")));
    bash(&w, "PostToolUse", r#"bun wisp farm balance "wren expert 120" --wait"#, &farm, "first");
    assert!(denied(&bash(&w, "PreToolUse", r#"cd ts && bun wisp farm balance "wren expert 120""#, &farm, "second")));
    assert!(!denied(&bash(&w, "PreToolUse", r#"bun wisp farm balance "wren expert 40" --wait"#, &farm, "second")));
}

#[test]
fn a_heavy_suite_runs_one_copy_per_worktree() {
    let w = world();
    let suite = w.root.join("suite/ts");
    let other = w.root.join("other");
    std::fs::create_dir_all(&suite).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let mut child = Command::new("bash")
        .args(["-c", "sleep 30; :", "bun", "scripts/lua-tests.ts"])
        .current_dir(&suite)
        .spawn()
        .unwrap();
    let parent = suite.parent().unwrap();
    let held = bash(&w, "PreToolUse", "cd ts && bun scripts/lua-tests.ts", parent, "process");
    let elsewhere = bash(&w, "PreToolUse", "bun scripts/lua-tests.ts", &other, "process");
    let stopping = bash(&w, "PreToolUse", "pkill -f scripts/lua-tests.ts", parent, "process");
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(denied(&held), "{held:?}");
    assert!(!denied(&elsewhere));
    assert!(!denied(&stopping));
}

#[test]
fn stop_refuses_an_ending_over_a_passing_unlanded_check_but_not_mid_box_or_checkpoint() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    std::fs::write(game.join("arc.ts"), "c\n").unwrap();
    bash(&w, "PostToolUse", "bun test -t arc", &game, "land");
    assert!(stop(&w, "Tests pass on the arc fix.", "land"));
    assert!(stop(&w, "Done: tests pass on the arc fix; land the change with safe-push --to main.", "land"));
    run(&w, json!({"hook_event_name": "UserPromptSubmit", "session_id": "mid-box", "prompt": "Read and do the box brief at brief-desktop-userns.md"}));
    assert!(!stop(&w, "I've created the private-desktop-userns worktree from origin/main. The fix is still outstanding; I haven't yet tested or landed it.", "mid-box"));
    assert!(!stop(&w, "Not done: Stop: capacity. Checkpoint saved; the parent owns the exclusive-capacity dependency.", "dependency-checkpoint"));
}

#[test]
fn an_orchestrator_hands_checks_and_log_digging_to_workers_but_heredoc_text_is_inert() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    run(&w, json!({"hook_event_name": "PostToolUse", "session_id": "orchestrator", "cwd": game, "tool_name": "collaborationspawn_agent", "tool_input": {}, "tool_response": "{}"}));
    for command in [
        "python3 - <<'PY'\nfrom pathlib import Path\nPath('status.md').write_text('passed; bun test; cat worker.log')\nPY",
        "cat <<'REPORT' > status.md\npassed; bun test; cat worker.log\nREPORT",
    ] {
        assert!(!denied(&bash(&w, "PreToolUse", command, &game, "orchestrator")));
        bash(&w, "PostToolUse", command, &game, "orchestrator");
        assert!(!denied(&bash(&w, "PreToolUse", command, &game, "orchestrator")));
        assert!(denied(&bash(&w, "PreToolUse", &format!("{command}\nbun test"), &game, "orchestrator")));
    }
    assert!(denied(&bash(&w, "PreToolUse", "bun test", &game, "orchestrator")));
    assert!(denied(&bash(&w, "PreToolUse", "cat worker.log", &game, "orchestrator")));
    assert!(!denied(&bash(&w, "PreToolUse", "bun scripts/machine-capacity.mjs estimate --json -- cargo build --release", &game, "orchestrator")));
    assert!(denied(&bash(&w, "PreToolUse", "bun scripts/machine-capacity.mjs run --owner x --timeout-seconds 60 -- cargo build --release", &game, "orchestrator")));
}

fn tooling_box(surface: &str) -> String {
    format!("Item: north#51\nCategory: tooling\nSpec: exact\nScope: module\nSurface: {surface}\nVerify: local-test\nFiles: crates/north-core/src/guard/codex_conduct.rs\nETA: 20 min\nDone when: `cargo test codex_conduct` passes\n\nThe goal in full sentences.")
}

fn spawn(w: &World, message: &str) -> Option<String> {
    match run(w, json!({"hook_event_name": "PreToolUse", "session_id": "gate-lead", "cwd": w.root, "tool_name": "spawn_agent", "tool_input": {"message": message, "task_name": "t"}})) {
        Verdict::Deny(reason) => Some(reason),
        _ => None,
    }
}

#[test]
fn the_spawn_gate_routes_unqualified_codex_boxes_to_claude() {
    let w = world();
    let refused = |message: &str, why: &str| {
        let reason = spawn(&w, message).unwrap_or_default();
        assert!(reason.contains(&format!("route to Claude: {why}")), "{reason}");
    };
    refused(&tooling_box("ts"), "Surface ts");
    assert_eq!(spawn(&w, &tooling_box("shell")), None);
    refused(&tooling_box("ts").replace("Scope: module", "Scope: cross-module"), "Scope cross-module");
    let balance = tooling_box("assets").replace("Category: tooling", "Category: balance-tuning").replace("Verify: local-test", "Verify: visual");
    refused(&balance, "Category balance-tuning");
    let explored = balance.replace("Item: north#51\n", "Item: north#51\nExplore: codex\n");
    assert_eq!(spawn(&w, &explored), None);
    let review = "Item: north#51\nCategory: review\nReviews: claude (opus-high netcode plan)\nSpec: judged\nScope: cross-module\nSurface: lua\nVerify: none\n\nBreak this netcode plan.\n";
    assert_eq!(spawn(&w, review), None);
    refused(&review.replace("Reviews: claude (opus-high netcode plan)", "Reviews: codex"), "Reviews codex");
    refused(&review.replace("Reviews: claude (opus-high netcode plan)\n", ""), "Reviews missing");
    refused(&explored.replace("Surface: assets", "Surface: lua"), "Surface lua");
    assert_eq!(spawn(&w, "Run the entire operator task: fix the failing build and report."), None);
    assert_eq!(spawn(&w, "echo_probe"), None);
}

#[test]
fn a_spawn_brief_that_only_measures_or_runs_words_together_is_refused() {
    let w = world();
    assert!(spawn(&w, "Measure-only: profile the frame and report.").unwrap().contains("only measures"));
    assert_eq!(spawn(&w, "Measure-only: profile the frame, then land the fix."), None);
    assert!(spawn(&w, "Readthebriefcarefullyandthenfixtheparser now.").is_none());
    assert!(spawn(&w, "read thebriefcarefullyandthenfixtheparser now").unwrap().contains("runs words together"));
}

fn plan(w: &World, cwd: &Path, explanation: &str, step: &str) -> Verdict {
    run(w, json!({"hook_event_name": "PreToolUse", "session_id": "planner", "cwd": cwd, "tool_name": "update_plan", "tool_input": {"explanation": explanation, "plan": [{"step": step, "status": "pending"}]}}))
}

#[test]
fn a_first_plan_needs_its_checklist_quoted_goal_profile_and_eta() {
    let w = world();
    let north = w.root.join("code/north/main");
    std::fs::create_dir_all(&north).unwrap();
    run(&w, json!({"hook_event_name": "UserPromptSubmit", "session_id": "planner", "prompt": "Please port the behavior guard to Rust."}));
    let full = |profile: &str, eta: &str| format!("goal: \"port the behavior guard\"\nprofile: {profile}\ndone-when: tests pass\nextra checks: none\nworkers: none\neta: {eta}");
    let reason = |v: Verdict| if let Verdict::Deny(r) = v { r } else { String::new() };
    assert!(reason(plan(&w, &north, "goal: x", "Port it")).contains("missing: profile, done-when"));
    assert!(reason(plan(&w, &north, &full("prototype", "20 min"), "Port it")).contains("The profile here is 'tooling', not 'prototype'"));
    assert!(reason(plan(&w, &north, &full("tooling", "soon"), "Port it")).contains("`eta:` must be a number"));
    assert!(reason(plan(&w, &north, &full("tooling", "20 min").replace("port the behavior guard", "rewrite everything now"), "Port it")).contains("quote the request"));
    assert!(reason(plan(&w, &north, &full("tooling", "20 min"), "Add an independent review")).contains("adds an extra review"));
    assert_eq!(plan(&w, &north, &full("tooling", "20 min"), "Port it"), Verdict::Allow);
}

#[test]
fn prompts_add_the_correction_and_close_rules() {
    let w = world();
    let advice = |prompt: &str| match run(&w, json!({"hook_event_name": "UserPromptSubmit", "session_id": "p", "prompt": prompt})) {
        Verdict::Advise(text) => text,
        _ => String::new(),
    };
    assert!(advice("why are you adding a review again?").starts_with("Tom is correcting you."));
    assert!(advice("it's still open, close it").starts_with("Tom wants this closed."));
    assert_eq!(advice("<codex_internal_context>stop asking</codex_internal_context>"), "");
}

#[test]
fn stop_refuses_permission_narration_disclaimers_and_untried_escalation() {
    let w = world();
    assert!(stop(&w, "The parser is fixed. Should I land it?", "s"));
    assert!(stop(&w, "Fixed the parser. Next, I'll run the tests.", "s"));
    assert!(stop(&w, "Done: 4 tests pass. This does not prove the parser is total.", "s"));
    assert!(stop(&w, "Blocked: I need you to look at this.", "s"));
    assert!(!stop(&w, "Blocked: the build failed with exit code 2.", "s"));
    assert!(stop(&w, "The port is nearly done.", "s"));
    assert!(!stop(&w, "The port is nearly done: 2 of 5 left.", "s"));
    assert!(!stop(&w, "Done: the decision for Tom: should I buy it?", "s"));
    assert_eq!(super::codex_conduct::sentence_for_tests("One. Two!\n\nThree?  "), "Three?");
}

#[test]
fn a_rerun_takes_one_justified_case_per_code_state() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    bash(&w, "PostToolUse", "bun test", &game, "r");
    let reason = |v: Verdict| if let Verdict::Deny(r) = v { r } else { String::new() };
    assert!(reason(bash(&w, "PreToolUse", "bun test", &game, "r")).contains("Which case is it?"));
    assert!(reason(bash(&w, "PreToolUse", "CASE=C FACT=\"landing the arc fix soon\" bun test", &game, "r")).contains("Case C stops here"));
    assert!(reason(bash(&w, "PreToolUse", "CASE=A FACT=\"redo it\" bun test", &game, "r")).contains("FACT must name"));
    assert!(reason(bash(&w, "PreToolUse", "CASE=A FACT=\"to be really sure of it now\" bun test", &game, "r")).contains("can't rest on \"sure\""));
    assert_eq!(bash(&w, "PreToolUse", "CASE=A FACT=\"the rebuilt map changed at 9 AM\" bun test", &game, "r"), Verdict::Allow);
    assert!(reason(bash(&w, "PreToolUse", "CASE=A FACT=\"the rebuilt map changed at 9 AM\" bun test", &game, "r")).contains("already used its one justification"));
}

#[test]
fn issues_open_only_with_closable_bodies_and_one_open_at_a_time() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    let reason = |v: Verdict| if let Verdict::Deny(r) = v { r } else { String::new() };
    assert!(reason(bash(&w, "PreToolUse", "gh issue create --title t --body 'fix it'", &game, "i")).contains("no Done when list"));
    let body = "gh issue create --title t --body 'Done when:\n- [ ] every input parses\nNot required: speed'";
    assert!(reason(bash(&w, "PreToolUse", body, &game, "i")).contains("promises \"every\""));
    let good = "gh issue create --title t --body 'Done when:\n- [ ] 500 inputs parse\nNot required: speed'";
    assert_eq!(bash(&w, "PreToolUse", good, &game, "i"), Verdict::Allow);
    bash(&w, "PostToolUse", good, &game, "i");
    assert!(reason(bash(&w, "PreToolUse", good, &game, "i")).contains("opened 1 issues and closed 0"));
    let ticked = bash_response(&w, "PostToolUse", "gh issue view 12", &game, "i", "Exit code: 0\nOutput:\n- [x] 500 inputs parse\n");
    assert!(matches!(ticked, Verdict::Advise(ref text) if text.contains("Every box on issue 12 is ticked")), "{ticked:?}");
}

#[test]
fn a_new_scaffold_file_in_a_prototype_needs_a_justification() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    let patch = |name: &str| run(&w, json!({"hook_event_name": "PreToolUse", "session_id": "sc", "cwd": game, "tool_name": "apply_patch", "tool_input": {"command": format!("*** Begin Patch\n*** Add File: {name}\n+x\n*** End Patch")}}));
    assert!(matches!(patch("tapes.sha256"), Verdict::Deny(ref r) if r.contains("adds a sha256 file")));
    assert_eq!(patch("arc2.ts"), Verdict::Allow);
    assert!(denied(&bash(&w, "PreToolUse", ": justify scaffold C \"needed for later\"", &game, "sc")));
    assert_eq!(bash(&w, "PreToolUse", ": justify scaffold B \"bun build fails: missing manifest.json\"", &game, "sc"), Verdict::Allow);
    assert_eq!(patch("manifest.json"), Verdict::Allow);
    assert!(denied(&patch("manifest.json")));
}

#[test]
fn measurements_end_in_a_fix_after_two() {
    let w = world();
    let game = w.root.join("code/makgora/main");
    repo(&game, "arc.ts");
    for _ in 0..2 {
        assert_eq!(bash(&w, "PreToolUse", "hyperfine ./arc", &game, "m"), Verdict::Allow);
        bash(&w, "PostToolUse", "hyperfine ./arc", &game, "m");
    }
    assert!(matches!(bash(&w, "PreToolUse", "hyperfine ./arc", &game, "m"), Verdict::Deny(ref r) if r.contains("You've run 2 measurements")));
    assert_eq!(bash(&w, "PreToolUse", "CASE=B FACT=\"fix: cache the arc table lookups\" hyperfine ./arc", &game, "m"), Verdict::Allow);
    assert!(matches!(bash(&w, "PreToolUse", "hyperfine ./arc", &game, "m"), Verdict::Deny(ref r) if r.contains("fix: baseline already")));
    run(&w, json!({"hook_event_name": "PostToolUse", "session_id": "m", "cwd": game, "tool_name": "apply_patch", "tool_input": {}}));
    assert_eq!(bash(&w, "PreToolUse", "hyperfine ./arc", &game, "m"), Verdict::Allow);
}
