#![cfg(test)]

use super::coordinator_role::{ClaudeStart, Registered, World, decide};
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
    content: Option<&'static str>,
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
    fn lane_file_bytes(&self, _dir: &str, _file: &str) -> Option<Vec<u8>> {
        self.content.map(|text| text.as_bytes().to_vec())
    }
    fn claude_start(&self) -> Option<ClaudeStart> {
        self.start.map(|(nested, print)| ClaudeStart { nested, print, conversation: None })
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
    matches!(decide(world, input), Verdict::Deny(reason) if reason.contains("coordinator-role-guard") && !reason.contains("north fleet launch"))
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
    let main_edits = [
        tool("Edit", json!({"file_path": "/home/tom/code/north/main/crates/north-guard/src/guard/coordinator_role.rs", "old_string": "a", "new_string": "b"})),
        tool("Write", json!({"file_path": "/home/tom/code/north/main/docs/landing.md", "content": "x"})),
        tool("MultiEdit", json!({"file_path": "~/code/north/main/README.md", "edits": []})),
        tool("NotebookEdit", json!({"notebook_path": "/home/tom/code/north/main/notes.ipynb", "new_source": "x"})),
    ];
    for edit in &main_edits {
        assert!(denied(&coordinator(), edit), "{:?}", edit.tool_input);
    }
    let lane_edits = [
        tool("Edit", json!({"file_path": format!("{LANE}/crates/north-guard/src/guard/coordinator_role.rs"), "old_string": "a", "new_string": "b"})),
        tool("Write", json!({"file_path": format!("{LANE}/docs/landing.md"), "content": "x"})),
        tool("MultiEdit", json!({"file_path": "~/code/north/worktrees/lane/README.md", "edits": []})),
        tool("NotebookEdit", json!({"notebook_path": format!("{LANE}/notes.ipynb"), "new_source": "x"})),
    ];
    for edit in &lane_edits {
        assert_eq!(decide(&coordinator(), edit), Verdict::Allow, "a coordinator may edit its lane: {:?}", edit.tool_input);
        assert_eq!(decide(&worker(), edit), Verdict::Allow, "a worker may edit its lane: {:?}", edit.tool_input);
    }
    let open = ["bun run script.ts", "npm view left-pad", "nix develop -c true", "echo x > /tmp/note", "echo x 2>&1 | cat", "git apply --check /tmp/fix.diff", "go version", "north fleet list"];
    for command in open {
        let verdict = decide(&coordinator(), &bash(command));
        assert_eq!(verdict, Verdict::Allow, "{command}");
    }
}

#[test]
fn the_coordinator_has_no_diagnosis_budget_without_break_glass() {
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

const SEALED: &str = "sops:\n    lastmodified: '2026-11-15T00:00:00Z'\n    mac: ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]\n    version: 3.9.0\n";
const PLAINTEXT: &str = "token: fixture-not-a-credential\n";

#[test]
fn proxy_lands_its_own_config_lane() {
    let lane = |files: &'static [&'static str]| Fake { lane: Some(files), content: Some(SEALED), ..coordinator() };
    let landing = ["git commit -qm south", "safe-push --to main"];
    let allowed: [&'static [&'static str]; 5] = [
        &["agents/rules.toml"],
        &["secrets/south.yaml"],
        &["harness/defaults.toml"],
        &["agents/rules.toml", "agents/other.toml"],
        &["crates/north-guard/src/guard/coordinator_role.rs"],
    ];
    for files in allowed {
        for landing_command in landing {
            let command = format!("cd {LANE} && {landing_command}");
            assert_eq!(decide(&lane(files), &bash(&command)), Verdict::Allow, "{command} over {files:?}");
        }
    }
    for landing_command in landing {
        assert!(denied(&lane(&["README.md"]), &bash(landing_command)), "{landing_command} in main checkout");
    }
    let plaintext = Fake { lane: Some(&["secrets/plain.yaml"]), content: Some(PLAINTEXT), ..coordinator() };
    for landing_command in landing {
        let command = format!("cd {LANE} && {landing_command}");
        assert!(denied(&plaintext, &bash(&command)), "{command} over a plaintext secrets file");
    }
}
