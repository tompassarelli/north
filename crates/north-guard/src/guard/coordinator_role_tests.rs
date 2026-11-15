#![cfg(test)]

use super::host::{MessageReads, ReadStreak};
use super::coordinator_role::{Budgets, ClaudeStart, Registered, World, budget_verdict, decide, streak_verdict};
use super::coordinator_loads::harness_load;
use crate::hook_input::{HookInput, Verdict};
use serde_json::{Value, json};

const LANE: &str = "/home/tom/code/north/worktrees/lane";

#[derive(Default)]
struct Fake {
    role: Option<&'static str>,
    session: Option<&'static str>,
    registered: Option<&'static str>,
    account: Option<&'static str>,
    config: Option<&'static str>,
    start: Option<(bool, bool)>,
    lane: Option<&'static [&'static str]>,
    glass: bool,
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
    fn registered_coordinator(&self) -> Option<Registered> {
        (self.registered.is_some() || self.account.is_some()).then(|| Registered { conversation: self.registered.map(str::to_owned), account: self.account.map(str::to_owned) })
    }
    fn profile(&self, account: &str) -> Option<String> {
        Some(format!("/home/tom/.local/state/north/profiles/claude-{account}/"))
    }
    fn lane_files(&self, dir: &str) -> Option<Vec<String>> {
        self.lane.filter(|_| dir == LANE).map(|files| files.iter().map(|file| (*file).to_owned()).collect())
    }
    fn claude_start(&self) -> Option<ClaudeStart> {
        self.start.map(|(nested, print)| ClaudeStart { nested, print, conversation: None })
    }
    fn break_glass(&self) -> bool {
        self.glass
    }
    fn status_verb(&self, words: &[&str]) -> bool {
        matches!(words, ["land", "status", ..] | ["glance", ..])
    }
}

fn coordinator() -> Fake {
    Fake { role: Some("coordinator"), session: Some("coordinator"), ..Fake::default() }
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

fn reads(looks: usize, looking: bool) -> ReadStreak {
    ReadStreak { calls: 3, subject: "why is the laptop at load 87?".into(), shapes: vec![("ps".into(), 3)], message: MessageReads { looks, looking } }
}

const BUDGETS: Budgets = Budgets { looks: 25 };

#[test]
fn a_coordinator_over_its_read_budget_is_refused_and_its_turn_continues() {
    let interleaved = reads(26, true);
    assert_eq!(streak_verdict(&interleaved, 15), Verdict::Allow, "sends keep resetting the streak, so the streak limit alone never fires");
    let Verdict::Deny(reason) = budget_verdict(&interleaved, &BUDGETS) else { panic!("the 26th look of one message went on") };
    assert!(reason.contains("26 read-only calls") && reason.contains("coordinator_look_budget") && reason.contains("only Tom's next message does") && reason.contains("north work brief new") && reason.contains("north fleet launch"), "{reason}");
    assert!(reason.contains("your turn goes on") && !reason.contains("STOPPED"), "{reason}");
    assert_eq!(budget_verdict(&reads(25, true), &BUDGETS), Verdict::Allow, "the budget itself still runs");
    assert_eq!(budget_verdict(&reads(26, false), &BUDGETS), Verdict::Allow, "the launch that hands the work off is not a look");
    let Verdict::Deny(reason) = streak_verdict(&ReadStreak { calls: 16, ..reads(16, true) }, 15) else { panic!("the 16th read of a streak went on") };
    assert!(reason.contains("your turn goes on") && reason.contains("north fleet launch"), "{reason}");
    assert_eq!(streak_verdict(&ReadStreak { calls: 16, ..reads(16, false) }, 15), Verdict::Allow, "a launch over the streak is not refused");
}

#[test]
fn harness_required_loads_spend_no_coordinator_read_budget() {
    let load = |tool: &str, text: &str| harness_load("/home/tom", Some("/home/tom/.local/state/north/profiles/claude-d"), tool, text);
    for (tool, text) in [
        ("Skill", ""),
        ("Read", "/home/tom/.local/state/north/profiles/claude-d/skills/workers/references/briefs.md"),
        ("Read", "~/.claude/skills/workers/SKILL.md"),
        ("Bash", "north work brief new --category bug-known-cause --item north#1106"),
        ("Bash", "north fleet launch --help"),
        ("Bash", "/home/tom/.local/bin/north threads queue --help"),
    ] {
        assert!(load(tool, text), "{tool} {text} is the harness's own load");
    }
    for (tool, text) in [
        ("Read", "/home/tom/code/north/main/harness/skills/workers/SKILL.md"),
        ("Read", "/home/tom/.local/state/north/profiles/claude-d/skills/../../../fleet/n1/report.md"),
        ("Read", "/tmp/skills/x.md"),
        ("Grep", "/home/tom/.claude/skills/x"),
        ("Bash", "north fleet launch --help; ps aux"),
        ("Bash", "north --help | rg -n code"),
        ("Bash", "rg --help"),
        ("Bash", "ps aux"),
    ] {
        assert!(!load(tool, text), "{tool} {text} is a look");
    }
}

#[test]
fn the_coordinator_cannot_build_test_or_edit_a_repository() {
    let refused = [
        "bun test",
        "bun run build",
        "npm test",
        "pnpm run build",
        "yarn build",
        "make all",
        "just check",
        "pytest -q",
        "go test ./...",
        "nix build .#north",
        "nix flake check",
        "cargo run -p north-guard",
        "echo x > ~/code/north/main/README.md",
        "printf x >> /home/tom/code/operator/main/agents/x.md",
        "cat notes | tee ~/code/north/main/docs/x.md",
        "git apply /tmp/fix.diff",
        "patch -p1 < /tmp/fix.diff",
        "ssh gex45 'cd ~/code/north/main && bun test'",
    ];
    for command in refused {
        assert!(denied(&coordinator(), &bash(command)), "{command}");
        assert_eq!(decide(&worker(), &bash(command)), Verdict::Allow, "a worker may run {command}");
    }
    let open = ["bun run script.ts", "npm view left-pad", "nix develop -c true", "echo x > /tmp/note", "echo x 2>&1 | cat", "git apply --check /tmp/fix.diff", "go version", "north fleet list"];
    for command in open {
        let verdict = decide(&coordinator(), &bash(command));
        assert_eq!(verdict, Verdict::Allow, "{command}");
    }
}

#[test]
fn the_coordinator_has_no_diagnosis_budget_without_break_glass() {
    let diagnosis = [
        bash("ssh nexus 'cd ~/code/north/main && strace -f -e trace=openat north fleet launch --provider opencode x 2>&1 | tail'"),
        bash("ssh nexus journalctl --user -u north-dispatch -n 50"),
        bash("ssh gex45 uptime"),
        bash("ssh -o BatchMode=yes cpx62b 'north fleet tree; ps aux'"),
        bash("ssh nexus"),
        bash("scp nexus:/tmp/launch.log /tmp/"),
        bash("journalctl --user -u north-lander -n 40"),
        bash("ps -eo pid,pcpu,args --sort=-pcpu | head"),
        bash("date '+%I:%M %p'; ps aux | head"),
        bash("top -bn1 | head -20"),
        bash("strace -c true"),
        bash("systemctl --user status north-dispatch"),
        bash("cat /proc/123/cmdline"),
        bash("rg -n deepseek ~/code/north/main/crates/north-providers"),
        bash("rg -n deepseek"),
        bash("sed -n 1,20p crates/north-guard/src/lib.rs"),
        bash("cd /tmp && cat /home/tom/code/north/main/harness/defaults.toml"),
        bash("git show HEAD~1"),
        bash("git -C /home/tom/code/north/main log -p -3 origin/main"),
        bash("bash -c 'grep -r opencode ~/code/north/main/src'"),
        bash("echo $(cat ~/code/north/main/Cargo.toml)"),
        tool("Read", json!({"file_path": "/home/tom/code/north/main/crates/north-guard/src/guard/coordinator_role.rs"})),
        tool("Grep", json!({"pattern": "deepseek"})),
        tool("Glob", json!({"pattern": "**/*.rs", "path": "/home/tom/code/north/main/crates"})),
    ];
    for input in &diagnosis {
        let Verdict::Deny(reason) = decide(&coordinator(), input) else { panic!("the coordinator diagnosed outside break-glass: {:?}", input.tool_input) };
        assert!(reason.contains("coordinator-reach-guard") && reason.contains("north work brief new") && reason.contains("`Item:`") && reason.contains("`Done when:`") && reason.contains("north fleet launch"), "{reason}");
        assert_eq!(decide(&worker(), input), Verdict::Allow, "a worker may {:?}", input.tool_input);
        assert_eq!(decide(&Fake { glass: true, ..coordinator() }, input), Verdict::Allow, "break-glass lifts {:?}", input.tool_input);
    }
    let open = [
        bash("north fleet send n1106 'go'"),
        bash("north gate open --session x --question q --option a --option b --recommend 1 --ask-once cost --amount '$20 a month'"),
        bash("north fleet tree | rg -v exited | head -40"),
        bash("cd /home/tom/code/north/main && north fleet tree 2>&1 | cut -c1-200 | sed -n 1,60p"),
        bash("north land status"),
        bash("gh issue view 1106 -R tompassarelli/north"),
        bash("gh issue edit 1106 -R tompassarelli/north --body-file /tmp/coordinator/body.md"),
        bash("git -C /home/tom/code/north/main log --oneline -10 origin/main"),
        bash("git -C ~/code/north/main fetch -q origin"),
        bash("cat /tmp/coordinator/brief.md"),
        bash("cat ~/.local/state/north/fleet/n1106/report.md"),
        bash("rg -n Done ~/.local/state/north/coordinator/notes.md"),
        bash("ssh nexus north fleet tree"),
        bash("ssh -T nexus -- 'north land status | head -20'"),
        bash("ssh gex45 'north glance'"),
        bash("uptime"),
        tool("Read", json!({"file_path": "/tmp/coordinator/brief.md"})),
        tool("Read", json!({"file_path": "/home/tom/.local/state/north/fleet/n1106/report.md"})),
        tool("Write", json!({"file_path": "/tmp/coordinator/brief.md", "content": "Item: north#1106 x"})),
    ];
    for input in &open {
        assert_eq!(decide(&coordinator(), input), Verdict::Allow, "{:?}", input.tool_input);
    }
}
