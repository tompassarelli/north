#![cfg(test)]

use super::proxy_role::{World, decide, shell_action};
use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    role: Option<&'static str>,
    session: Option<&'static str>,
    registered: Option<&'static str>,
    fleet: &'static str,
    ready: &'static str,
    seen: Option<&'static str>,
}

impl World for Fake {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/home/tom".into()),
            "NORTH_FLEET_ROLE" => self.role.map(str::to_owned),
            "NORTH_FLEET_SESSION" => self.session.map(str::to_owned),
            _ => None,
        }
    }
    fn registered_proxy(&self) -> Option<String> {
        self.registered.map(str::to_owned)
    }
    fn fleet(&self) -> Option<String> {
        Some(self.fleet.to_owned())
    }
    fn ready(&self) -> Option<String> {
        Some(self.ready.to_owned())
    }
    fn seen(&self, _session: &str, items: &str) -> bool {
        self.seen.is_some_and(|seen| seen == items)
    }
}

fn proxy() -> Fake {
    Fake { role: Some("proxy"), session: Some("proxy"), ..Fake::default() }
}

fn worker() -> Fake {
    Fake { role: Some("worker"), session: Some("n523"), ..Fake::default() }
}

fn tool(name: &str, input: Value) -> HookInput {
    let data = json!({"hook_event_name": "PreToolUse", "session_id": "s1", "cwd": "/home/tom/code/north/main", "tool_name": name, "tool_input": input});
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

fn bash(command: &str) -> HookInput {
    tool("Bash", json!({"command": command}))
}

fn denied(world: &Fake, input: &HookInput) -> bool {
    matches!(decide(world, input), Verdict::Deny(reason) if reason.contains("north fleet launch"))
}

fn implementation() -> Vec<HookInput> {
    vec![
        tool("Edit", json!({"file_path": "/home/tom/code/north/worktrees/x/src/lib.rs", "old_string": "a", "new_string": "b"})),
        tool("Write", json!({"file_path": "~/code/south/main/agents/x.md", "content": "x"})),
        tool("Write", json!({"file_path": "src/main.rs", "content": "x"})),
        bash("git commit -m wip src/lib.rs"),
        bash("cd /home/tom/code/north/worktrees/x && git -C . commit --amend"),
        bash("safe-push --to main"),
        bash("FOO=1 /home/tom/.local/bin/safe-push --to main"),
        bash("cargo build -p north-guard"),
        bash("cargo +nightly test --lib"),
        bash("cargo clippy --tests"),
        bash("north capacity offload run --host gex45 -- 'cd /x && cargo test'"),
        bash("cargo --color always test"),
        bash("cargo c"),
        bash("git -c user.name=\"A B\" commit -m x"),
        bash("bash -c 'cargo test --lib'"),
        bash("ssh gex45 \"cd x && git commit -m y\""),
        bash("timeout 60 cargo test"),
        bash("env -u X cargo build"),
        tool("Agent", json!({"subagent_type": "worker", "description": "d", "prompt": "p"})),
        tool("Agent", json!({"description": "d", "prompt": "p"})),
    ]
}

#[test]
fn the_proxy_is_refused_every_implementation_action_with_the_launch_route() {
    for input in implementation() {
        assert!(denied(&proxy(), &input), "{:?}", input.data);
    }
}

#[test]
fn a_worker_session_is_allowed_every_implementation_action() {
    for input in implementation() {
        assert_eq!(decide(&worker(), &input), Verdict::Allow, "{:?}", input.data);
        assert_eq!(decide(&Fake::default(), &input), Verdict::Allow, "{:?}", input.data);
    }
}

#[test]
fn the_proxy_keeps_its_reads_routing_and_read_only_subagents() {
    for input in [
        bash("git log --oneline -5 && git status"),
        bash("north fleet launch --task north#1 --account a 'cargo test the guard'"),
        bash("north threads ready | head"),
        bash("gh issue view 523 --json body"),
        bash("rg -n cargo crates/north-guard/src"),
        bash("cat docs/safe-push.md"),
        bash("north fleet launch --task x#1 --brief \"Verify: cargo check --tests\nthen `cargo test`; done\""),
        bash("gh issue comment 1 --body \"ran `cargo test`; ok\""),
        bash("rg -n \"git commit\" docs"),
        bash("cat x | rg \"cargo test\" -"),
        bash("ls bin/safe-push && command -v safe-push"),
        tool("Read", json!({"file_path": "/home/tom/code/north/main/Cargo.toml"})),
        tool("Write", json!({"file_path": "/home/tom/.local/state/agents/handoffs/x.md", "content": "x"})),
        tool("Agent", json!({"subagent_type": "Explore", "description": "d", "prompt": "p"})),
        tool("Agent", json!({"subagent_type": "Plan", "description": "d", "prompt": "p"})),
    ] {
        assert_eq!(decide(&proxy(), &input), Verdict::Allow, "{:?}", input.data);
    }
}

#[test]
fn identity_comes_from_the_agent_then_the_launch_role_then_the_registry() {
    let commit = |extra: Value| {
        let mut data = json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "git commit -m x"}});
        for (key, value) in extra.as_object().unwrap() {
            data[key] = value.clone();
        }
        HookInput::parse(data.to_string().as_bytes()).unwrap()
    };
    assert!(denied(&worker(), &commit(json!({"agent_type": "proxy"}))));
    assert_eq!(decide(&proxy(), &commit(json!({"agent_type": "worker", "agent_id": "a1"}))), Verdict::Allow);
    assert_eq!(decide(&proxy(), &commit(json!({"agent_id": "a1"}))), Verdict::Allow);
    let unbound = Fake { registered: Some("s1"), ..Fake::default() };
    assert!(denied(&unbound, &commit(json!({}))));
    let other = Fake { registered: Some("s2"), ..Fake::default() };
    assert_eq!(decide(&other, &commit(json!({}))), Verdict::Allow);
    let fleet_worker = Fake { session: Some("n523"), registered: Some("s1"), ..Fake::default() };
    assert_eq!(decide(&fleet_worker, &commit(json!({}))), Verdict::Allow);
}

#[test]
fn shell_actions_name_what_was_refused() {
    assert_eq!(shell_action("git commit -m x").as_deref(), Some("make a git commit"));
    assert_eq!(shell_action("timeout 900 safe-push --to main").as_deref(), Some("land through safe-push"));
    assert_eq!(shell_action("cargo check --tests").as_deref(), Some("run cargo check"));
    assert_eq!(shell_action("cargo fmt --check"), None);
    assert_eq!(shell_action("git commit-tree x"), None);
    assert_eq!(shell_action("cat <<'EOF'\ncargo test\nEOF"), None);
}

fn stop(active: bool) -> HookInput {
    let data = json!({"hook_event_name": "Stop", "session_id": "s1", "stop_hook_active": active});
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

const READY: &str = "north#523\tProxy role guard\nnorth#524\tOther\n";

#[test]
fn an_idle_fleet_with_ready_items_sends_the_proxy_back_to_staff_them() {
    let idle = Fake { fleet: r#"[{"name":"proxy","conversation":"s1","status":"working"},{"name":"w","conversation":"s9","status":"parked"}]"#, ready: READY, ..proxy() };
    match decide(&idle, &stop(false)) {
        Verdict::Deny(reason) => assert!(reason.contains("2 ready items (north#523, north#524)") && reason.contains("north fleet launch"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(decide(&idle, &stop(true)), Verdict::Allow);
    let flagged = Fake { seen: Some("north#523\nnorth#524"), fleet: idle.fleet, ready: READY, ..proxy() };
    assert_eq!(decide(&flagged, &stop(false)), Verdict::Allow);
    let busy = Fake { fleet: r#"[{"name":"w","conversation":"s9","status":"working"}]"#, ready: READY, ..proxy() };
    assert_eq!(decide(&busy, &stop(false)), Verdict::Allow);
    let empty = Fake { fleet: "[]", ready: "", ..proxy() };
    assert_eq!(decide(&empty, &stop(false)), Verdict::Allow);
    let idle_worker = Fake { fleet: "[]", ready: READY, ..worker() };
    assert_eq!(decide(&idle_worker, &stop(false)), Verdict::Allow);
}
