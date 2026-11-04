use serde_json::{Value, json};

use super::calls::Kind::{self, *};
use super::{VerbKinds, classify, read, render, shape, streaks};

const VERBS: VerbKinds = VerbKinds::new(&[
    ("", Status),
    ("fleet", Status),
    ("fleet launch", Delegate),
    ("fleet send", ChildMessage),
    ("fleet stop", Bookkeeping),
    ("gate", TomFacing),
    ("brief", Brief),
    ("land", Landing),
    ("land scan", Diagnosis),
    ("flow", Status),
    ("grounding", Diagnosis),
]);

fn bash(command: &str) -> Kind {
    classify("Bash", &json!({ "command": command }), &VERBS)
}

#[test]
fn north_commands_take_the_kind_their_verb_declares() {
    let cases: &[(&str, Kind)] = &[
        ("north fleet launch --name w1 --task 'Item: x'", Delegate),
        ("north fleet send w1 'rebase on main'", ChildMessage),
        ("north gate open --question q --option a", TomFacing),
        ("north brief new --category feature", Brief),
        ("north fleet stop w1", Bookkeeping),
        ("north land enqueue --branch b", Landing),
        ("north land scan secrets a..b", Diagnosis),
        ("north fleet show w1", Status),
        ("cd ~/code/north/main && NORTH_X=1 ~/.local/bin/north flow", Status),
        ("north grounding query x", Diagnosis),
        ("north", Status),
        ("north no-such-verb", Diagnosis),
    ];
    for (command, kind) in cases {
        assert_eq!(bash(command), *kind, "{command}");
    }
}

#[test]
fn foreign_commands_follow_the_ordered_rules() {
    let cases: &[(&str, Kind)] = &[
        ("threads queue follow north#1", Delegate),
        ("threads tick north#1 2", Bookkeeping),
        ("threads list", Status),
        ("date '+%I:%M %p'", TomFacing),
        ("cat > /tmp/brief.txt <<'EOF'\nItem: north#112 box P2\nEOF", Brief),
        ("sleep 60", Monitoring),
        ("while true; do north fleet show w1; sleep 5; done", Monitoring),
        ("nohup cargo build &", Monitoring),
        ("cat > /tmp/watch.sh <<'EOF'\necho hi\nEOF", Monitoring),
        ("gh issue comment 112 --body done", Bookkeeping),
        ("git merge-base --is-ancestor abc origin/main", Bookkeeping),
        ("git -C ~/code/north/main worktree remove ../worktrees/x", Bookkeeping),
        ("git branch -D lane", Bookkeeping),
        ("echo 'landed x' >> ~/scratch/queue.md", Bookkeeping),
        ("bin/safe-push --to main", Landing),
        ("git -C lane commit -m msg", Landing),
        ("git push origin lane", Landing),
        ("sed -i 's/a/b/' src/x.rs", Landing),
        ("perl -pi -e 's/a/b/' src/x.rs", Landing),
        ("tail -50 ~/.local/state/safe-push/land-check-7.log", LandingLogs),
        ("ls ~/.local/state/north/land-queue/abc", LandingLogs),
        ("journalctl --user -u north-land-serve -n 40", LandingLogs),
        ("journalctl --user -n 40", Status),
        ("ps aux | rg claude", Status),
        ("gh run list --limit 3", Status),
        ("systemctl --user status north-watch", Status),
        ("rg -n foo src", Diagnosis),
        ("git log --oneline -5", Diagnosis),
        ("cargo test -p north-core", Diagnosis),
        ("ssh nexus uptime", Diagnosis),
        ("sed -n 1,20p src/x.rs", Diagnosis),
    ];
    for (command, kind) in cases {
        assert_eq!(bash(command), *kind, "{command}");
    }
}

#[test]
fn the_first_kind_in_rule_order_wins() {
    assert_eq!(bash("north fleet show w1 && north fleet launch --name w2"), Delegate);
    assert_eq!(bash("git log -1 && bin/safe-push --to main"), Landing);
    assert_eq!(bash("north fleet send w1 hi; sleep 5"), ChildMessage);
}

#[test]
fn tools_other_than_bash_classify_by_name_and_target() {
    let cases: &[(&str, Value, Kind)] = &[
        ("Agent", json!({ "subagent_type": "worker" }), Delegate),
        ("SendMessage", json!({ "to": "w1" }), ChildMessage),
        ("ListAgents", json!({}), ChildMessage),
        ("Monitor", json!({}), Monitoring),
        ("TaskStop", json!({}), Monitoring),
        ("ScheduleWakeup", json!({}), Monitoring),
        ("Write", json!({ "file_path": "/tmp/b.txt", "content": "Item: north#1" }), Brief),
        ("Write", json!({ "file_path": "/tmp/poll.sh", "content": "sleep 1" }), Monitoring),
        ("Edit", json!({ "file_path": "src/x.rs" }), Landing),
        ("Read", json!({ "file_path": "/home/t/.local/state/safe-push/land-check-1.log" }), LandingLogs),
        ("Read", json!({ "file_path": "src/x.rs" }), Diagnosis),
        ("ToolSearch", json!({}), Diagnosis),
    ];
    for (tool, input, kind) in cases {
        assert_eq!(classify(tool, input, &VERBS), *kind, "{tool} {input}");
    }
}

#[test]
fn a_verb_with_no_declared_kind_is_reported() {
    assert_eq!(VERBS.undeclared(["fleet", "gate", "proxy", "land"]), vec!["proxy"]);
    assert_eq!(VerbKinds::new(&[("fleet", Status), ("gone sub", Status)]).strays(&["fleet"]), vec!["gone sub"]);
}

#[test]
fn shapes_name_the_verb_or_the_first_two_words_of_each_stage() {
    let shape_of = |command: &str| shape("Bash", &json!({ "command": command }));
    assert_eq!(shape_of("north fleet show w1 --json"), "north fleet show");
    assert_eq!(shape_of("cd ~/code/north/main && git merge-base --is-ancestor a b"), "git merge-base");
    assert_eq!(shape_of("journalctl -u north-land | tail -5"), "journalctl -u | tail -5");
    assert_eq!(shape("Read", &json!({ "file_path": "x" })), "Read");
}

#[test]
fn streaks_run_between_hand_offs() {
    let kinds = [Diagnosis, Status, Delegate, Diagnosis, ChildMessage, TomFacing, Landing, Landing, Landing];
    assert_eq!(streaks(&kinds), vec![2, 1, 3]);
}

#[test]
fn the_readout_counts_main_thread_calls_once() {
    let line = |sidechain: bool, id: &str, name: &str, input: Value| {
        json!({ "type": "assistant", "isSidechain": sidechain, "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }).to_string()
    };
    let transcript = [
        line(false, "a", "Bash", json!({ "command": "north fleet show w1" })),
        line(false, "a", "Bash", json!({ "command": "north fleet show w1" })),
        line(false, "b", "Agent", json!({})),
        line(true, "c", "Bash", json!({ "command": "rg x" })),
        json!({ "type": "user", "message": { "content": "hi" } }).to_string(),
        line(false, "d", "Read", json!({ "file_path": "src/x.rs" })),
    ]
    .join("\n");
    let readout = read(&transcript, &VERBS);
    assert_eq!(readout.kinds, vec![Status, Delegate, Diagnosis]);
    assert_eq!(readout.sidechain, 1);
    let text = render(&readout);
    assert!(text.starts_with("calls: 3 (subagent calls skipped: 1)"), "{text}");
    assert!(text.contains("- status checks: 1"), "{text}");
    assert!(text.contains("streaks: 2, median 1, 90th percentile 1, longest 1; 0 reached 5, 0 reached 15"), "{text}");
}
