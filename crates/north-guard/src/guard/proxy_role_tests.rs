#![cfg(test)]

use super::host::{MessageReads, ReadStreak};
use super::proxy_role::{Budgets, ClaudeStart, Registered, World, budget_verdict, decide, harness_load, lane_changes, streak_verdict};
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
    fn registered_proxy(&self) -> Option<Registered> {
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
fn the_proxy_is_refused_every_implementation_action_and_a_question_box_that_would_block_its_session() {
    for input in implementation() {
        assert!(denied(&proxy(), &input), "{:?}", input.data);
    }
    let ask = tool("AskUserQuestion", json!({"questions": [{"question": "Give gex45 a key nexus accepts?", "options": [{"label": "yes"}, {"label": "no"}]}]}));
    let Verdict::Deny(reason) = super::question_box::decide(&proxy(), &ask) else { panic!("a proxy's question box blocks its whole session and must be denied") };
    assert!(reason.contains("north gate open") && reason.contains("--default-after") && reason.contains("multiple-choice question in text"), "{reason}");
    assert_eq!(super::question_box::decide(&worker(), &ask), Verdict::Allow, "a worker's question becomes a gate through its host");
    a_lane_of_one_config_or_doc_file_lands_and_any_other_lane_is_refused();
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

fn a_lane_of_one_config_or_doc_file_lands_and_any_other_lane_is_refused() {
    let landing = [
        bash(&format!("cd {LANE} && git commit -qam x")),
        bash(&format!("cd {LANE}; safe-push --to main")),
        bash(&format!("git -C {LANE} commit -m x")),
        bash("cd ../worktrees/lane && git add -A && git commit -m x && safe-push --to main"),
        bash("cd ~/code/north/worktrees/lane && git commit -m x"),
        bash(&format!("cat /tmp/x.output; cd {LANE} && git commit -qam x && git log -1 --oneline")),
        bash(&format!("cd /home/tom/code/north/main && git fetch -q && cd {LANE} && sed -i 1d docs/a.md && git commit -qam x")),
        bash(&format!("cd {LANE}; git fetch -q origin && git rebase -q origin/main 2>&1 | tail -1; safe-push --to main")),
    ];
    let lane = |files| Fake { lane: Some(files), ..proxy() };
    for allowed in [&["harness/defaults.toml"][..], &["docs/features.md"], &["harness/skills/github/SKILL.md"], &["harness/skills/github/references/x.md"]] {
        for input in &landing {
            assert_eq!(decide(&lane(allowed), input), Verdict::Allow, "{allowed:?} {:?}", input.data);
        }
    }
    for refused in [&["src/lib.rs"][..], &["harness/defaults.toml", "docs/a.md"], &["harness/code/x.rs"], &["harness/skills/github/agents/x.md"], &["docs/../src/a.rs"], &[]] {
        for input in &landing {
            assert!(denied(&lane(refused), input), "{refused:?} {:?}", input.data);
        }
    }
    assert!(denied(&proxy(), &landing[0]), "an unreadable lane is refused");
    for elsewhere in ["git commit -m x", "safe-push --to main", "cd /home/tom/code/north/main && git commit -m x", "git -C /home/tom/code/north/main commit -m x"] {
        assert!(denied(&lane(&["harness/defaults.toml"]), &bash(elsewhere)), "{elsewhere} acts in the session directory");
    }
    for elsewhere in [format!("cd {LANE} && git commit -qam x && cd /home/tom/code/north/main && git commit -qam y"), "cd /home/tom/code/north/main && git commit -qam y".to_owned()] {
        assert!(denied(&lane(&["harness/defaults.toml"]), &bash(&elsewhere)), "{elsewhere} lands outside the lane");
    }
    assert_eq!(decide(&proxy(), &bash("git commit --allow-empty --dry-run -m probe")), Verdict::Allow, "a dry run commits nothing");
    for unresolved in [format!("cd \"{LANE}\" && git commit -m x"), "cd $D && git commit -m x".to_owned(), format!("cd {LANE} && cd .. && git commit -m x"), format!("git -C {LANE} -C /tmp commit -m x"), format!("cd {LANE} && git -C /tmp commit -m x")] {
        assert!(denied(&lane(&["harness/defaults.toml"]), &bash(&unresolved)), "{unresolved}");
    }
    assert!(denied(&lane(&["harness/defaults.toml"]), &bash(&format!("cd {LANE} && git commit -m x && cargo test"))), "only the landing steps are allowed");
    a_lane_behind_origin_main_lists_only_the_files_it_changed();
}

fn a_lane_behind_origin_main_lists_only_the_files_it_changed() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().to_str().unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git").args(["-C", dir, "-c", "user.email=t@example", "-c", "user.name=t"]).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}");
    };
    let write = |name: &str| {
        std::fs::create_dir_all(root.path().join(name).parent().unwrap()).unwrap();
        std::fs::write(root.path().join(name), name).unwrap();
    };
    git(&["init", "-q", "-b", "main"]);
    write("base.txt");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    git(&["branch", "lane"]);
    write("src/moved.rs");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "main moved"]);
    git(&["update-ref", "refs/remotes/origin/main", "main"]);
    git(&["checkout", "-q", "lane"]);
    write("docs/spend-rule.md");
    assert_eq!(lane_changes(dir), Some(vec!["docs/spend-rule.md".to_owned()]), "untracked");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "lane"]);
    assert_eq!(lane_changes(dir), Some(vec!["docs/spend-rule.md".to_owned()]), "committed");
}

fn reads(looks: usize, looking: bool) -> ReadStreak {
    ReadStreak { calls: 3, subject: "why is the laptop at load 87?".into(), shapes: vec![("ps".into(), 3)], message: MessageReads { looks, looking } }
}

const BUDGETS: Budgets = Budgets { looks: 25 };

#[test]
fn a_proxy_over_its_read_budget_is_refused_and_its_turn_continues() {
    let interleaved = reads(26, true);
    assert_eq!(streak_verdict(&interleaved, 15), Verdict::Allow, "sends keep resetting the streak, so the streak limit alone never fires");
    let Verdict::Deny(reason) = budget_verdict(&interleaved, &BUDGETS) else { panic!("the 26th look of one message went on") };
    assert!(reason.contains("26 read-only calls") && reason.contains("proxy_look_budget") && reason.contains("only Tom's next message does") && reason.contains("north work brief new") && reason.contains("north fleet launch"), "{reason}");
    assert!(reason.contains("your turn goes on") && !reason.contains("STOPPED"), "{reason}");
    assert_eq!(budget_verdict(&reads(25, true), &BUDGETS), Verdict::Allow, "the budget itself still runs");
    assert_eq!(budget_verdict(&reads(26, false), &BUDGETS), Verdict::Allow, "the launch that hands the work off is not a look");
    let Verdict::Deny(reason) = streak_verdict(&ReadStreak { calls: 16, ..reads(16, true) }, 15) else { panic!("the 16th read of a streak went on") };
    assert!(reason.contains("your turn goes on") && reason.contains("north fleet launch"), "{reason}");
    assert_eq!(streak_verdict(&ReadStreak { calls: 16, ..reads(16, false) }, 15), Verdict::Allow, "a launch over the streak is not refused");
}

#[test]
fn harness_required_loads_spend_no_proxy_read_budget() {
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
fn a_path_inside_an_issue_body_is_not_a_proxy_read() {
    let body = "gh issue create -R tompassarelli/north --title 'EVALS' --body \"$(cat <<'EOF'\nCause: it does not read X\nrg -n x /home/tom/code/north/main/docs/EVALS.md\ncat ~/code/north/main/Cargo.toml | head\nEOF\n)\"";
    let plain = "gh issue create -R tompassarelli/north --title t --body-file - <<'EOF'\nsee: rg -n x /home/tom/code/north/main/docs/EVALS.md\nEOF";
    let quoted = "gh issue comment 1106 --body 'the guard refused rg /home/tom/code/north/main/docs/EVALS.md; cat /home/tom/code/north/main/Cargo.toml'";
    for command in [body, plain, quoted] {
        assert_eq!(decide(&proxy(), &bash(command)), Verdict::Allow, "{command}");
    }
    for command in ["rg -n x /home/tom/code/north/main/docs/EVALS.md", "bash <<'EOF'\nrg -n x /home/tom/code/north/main/docs/EVALS.md\nEOF", "cat <<'EOF' | bash\ncat /home/tom/code/north/main/Cargo.toml\nEOF", "gh issue comment 1 --body \"$(rg -n x /home/tom/code/north/main/docs/EVALS.md)\""] {
        assert!(denied_reach(&proxy(), &bash(command)), "{command}");
    }
}

fn denied_reach(world: &Fake, input: &HookInput) -> bool {
    matches!(decide(world, input), Verdict::Deny(reason) if reason.contains("proxy-reach-guard"))
}

#[test]
fn the_proxy_cannot_build_test_or_edit_a_repository() {
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
        "printf x >> /home/tom/code/south/main/agents/x.md",
        "cat notes | tee ~/code/north/main/docs/x.md",
        "git apply /tmp/fix.diff",
        "patch -p1 < /tmp/fix.diff",
        "ssh gex45 'cd ~/code/north/main && bun test'",
    ];
    for command in refused {
        assert!(denied(&proxy(), &bash(command)), "{command}");
        assert_eq!(decide(&worker(), &bash(command)), Verdict::Allow, "a worker may run {command}");
    }
    let open = ["bun run script.ts", "npm view left-pad", "nix develop -c true", "echo x > /tmp/note", "echo x 2>&1 | cat", "git apply --check /tmp/fix.diff", "go version", "north fleet list"];
    for command in open {
        let verdict = decide(&proxy(), &bash(command));
        assert_eq!(verdict, Verdict::Allow, "{command}");
    }
}

#[test]
fn the_proxy_has_no_diagnosis_budget_without_break_glass() {
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
        tool("Read", json!({"file_path": "/home/tom/code/north/main/crates/north-guard/src/guard/proxy_role.rs"})),
        tool("Grep", json!({"pattern": "deepseek"})),
        tool("Glob", json!({"pattern": "**/*.rs", "path": "/home/tom/code/north/main/crates"})),
    ];
    for input in &diagnosis {
        let Verdict::Deny(reason) = decide(&proxy(), input) else { panic!("the proxy diagnosed outside break-glass: {:?}", input.tool_input) };
        assert!(reason.contains("proxy-reach-guard") && reason.contains("north work brief new") && reason.contains("`Item:`") && reason.contains("`Done when:`") && reason.contains("north fleet launch"), "{reason}");
        assert_eq!(decide(&worker(), input), Verdict::Allow, "a worker may {:?}", input.tool_input);
        assert_eq!(decide(&Fake { glass: true, ..proxy() }, input), Verdict::Allow, "break-glass lifts {:?}", input.tool_input);
    }
    let open = [
        bash("north fleet launch --provider claude --task north#1106 --box x 'brief'"),
        bash("north fleet send n1106 'go'"),
        bash("north gate open --session x --question q --option a --option b --recommend 1"),
        bash("north fleet tree | rg -v exited | head -40"),
        bash("cd /home/tom/code/north/main && north fleet tree 2>&1 | cut -c1-200 | sed -n 1,60p"),
        bash("north land status"),
        bash("gh issue view 1106 -R tompassarelli/north"),
        bash("gh issue edit 1106 -R tompassarelli/north --body-file /tmp/proxy/body.md"),
        bash("git -C /home/tom/code/north/main log --oneline -10 origin/main"),
        bash("git -C ~/code/north/main fetch -q origin"),
        bash("cat /tmp/proxy/brief.md"),
        bash("cat ~/.local/state/north/fleet/n1106/report.md"),
        bash("rg -n Done ~/.local/state/north/proxy/notes.md"),
        bash("ssh nexus north fleet tree"),
        bash("ssh -T nexus -- 'north land status | head -20'"),
        bash("ssh gex45 'north glance'"),
        bash("uptime"),
        tool("Read", json!({"file_path": "/tmp/proxy/brief.md"})),
        tool("Read", json!({"file_path": "/home/tom/.local/state/north/fleet/n1106/report.md"})),
        tool("Write", json!({"file_path": "/tmp/proxy/brief.md", "content": "Item: north#1106 x"})),
    ];
    for input in &open {
        assert_eq!(decide(&proxy(), input), Verdict::Allow, "{:?}", input.tool_input);
    }
}

