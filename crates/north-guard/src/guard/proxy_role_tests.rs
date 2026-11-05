#![cfg(test)]

use super::proxy_role::{ClaudeStart, Registered, World, decide};
use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    role: Option<&'static str>,
    session: Option<&'static str>,
    registered: Option<&'static str>,
    account: Option<&'static str>,
    config: Option<&'static str>,
    start: Option<(bool, bool)>,
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
            "CLAUDE_CONFIG_DIR" => self.config.map(str::to_owned),
            _ => None,
        }
    }
    fn registered_proxy(&self) -> Option<Registered> {
        (self.registered.is_some() || self.account.is_some()).then(|| Registered { conversation: self.registered.map(str::to_owned), account: self.account.map(str::to_owned) })
    }
    fn profile(&self, account: &str) -> Option<String> {
        Some(format!("/home/tom/.local/state/north/profiles/claude-{account}/"))
    }
    fn claude_start(&self) -> Option<ClaudeStart> {
        self.start.map(|(nested, print)| ClaudeStart { nested, print })
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

#[test]
fn a_proxy_resumed_by_hand_on_its_profile_is_the_proxy_and_a_claude_started_from_its_shell_is_not() {
    let commit = bash("git commit -m x");
    let profile = Some("/home/tom/.local/state/north/profiles/claude-a");
    let by_hand = Fake { account: Some("a"), config: profile, start: Some((false, false)), ..Fake::default() };
    assert!(denied(&by_hand, &commit));
    let scripted = Fake { start: Some((false, true)), ..by_hand };
    assert_eq!(decide(&scripted, &commit), Verdict::Allow);
    let elsewhere = Fake { config: Some("/home/tom/.local/state/north/profiles/claude-b"), start: Some((false, false)), ..by_hand };
    assert_eq!(decide(&elsewhere, &commit), Verdict::Allow);
    let unknown = Fake { start: None, ..by_hand };
    assert_eq!(decide(&unknown, &commit), Verdict::Allow);
    let nested = Fake { start: Some((true, true)), ..proxy() };
    assert_eq!(decide(&nested, &commit), Verdict::Allow);
    let nested_by_hand = Fake { start: Some((true, false)), ..by_hand };
    assert_eq!(decide(&nested_by_hand, &commit), Verdict::Allow);
    let launched = Fake { start: Some((false, true)), ..proxy() };
    assert!(denied(&launched, &commit));
}

#[test]
fn tom_resumed_window_on_another_profile_than_the_registered_proxy_is_his_window_but_not_the_proxy() {
    use super::proxy_role::is_tom_window;
    let input = tool("Bash", json!({"command": "git commit -m x"}));
    let claude_c = Some("/home/tom/.local/state/north/profiles/claude-c");
    let resumed = Fake { registered: Some("d11ef811"), account: Some("a"), config: claude_c, start: Some((false, false)), ..Fake::default() };
    assert!(is_tom_window(&resumed, &input));
    assert_eq!(decide(&resumed, &input), Verdict::Allow, "the code-work refusals stay with the registered proxy");
    assert!(!is_tom_window(&Fake { start: Some((false, true)), ..resumed }, &input), "a claude -p is not his window");
    let nested = Fake { registered: Some("d11ef811"), account: Some("a"), config: claude_c, start: Some((true, false)), ..Fake::default() };
    assert!(!is_tom_window(&nested, &input));
    let fleet_worker = Fake { registered: Some("d11ef811"), account: Some("a"), config: claude_c, start: Some((false, false)), ..worker() };
    assert!(!is_tom_window(&fleet_worker, &input));
    let unknown = Fake { start: None, ..Fake::default() };
    assert!(!is_tom_window(&unknown, &input));
}
