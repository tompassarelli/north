use serde_json::json;

use super::sequence::{Limits, recurring};
use super::steps::{command_of, steps};
use super::{Row, sessions};
use crate::ops::detectors::Detector;

const NOW: &str = "2026-11-05T12:00:00Z";

fn row(session: &str, id: i64, ts: &str, text: &str) -> serde_json::Value {
    json!({ "session": session, "agent": null, "title": null, "project": "north", "provider": "claude", "id": id, "ts": ts, "role": "tool", "tool": "Bash", "snippet": "", "path": "", "line": 1, "text": text })
}

fn repeated(session: &str, day: u32, noise: &str) -> Vec<serde_json::Value> {
    let ts = |minute: u32| format!("2026-11-{day:02}T10:{minute:02}:00Z");
    vec![
        row(session, 1, &ts(0), &format!("command={noise}\ndescription=Look around")),
        row(session, 2, &ts(1), "file_path=/home/user/north/Cargo.toml"),
        row(session, 3, &ts(2), "command=cd /home/user/north && depbox run -- nix develop -c cargo check -p north-core\ndescription=Check the crate"),
        row(session, 4, &ts(3), "command=git add -A"),
        row(session, 5, &ts(4), "command=git commit -m 'Land the lane'"),
        row(session, 6, &ts(5), "command=RUST_LOG=info git push origin HEAD:refs/heads/claude/land-x 2>&1"),
        row(session, 7, &ts(6), &format!("command={noise} --again")),
    ]
}

fn fixture() -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, noise) in ["ls -la", "rg -n tool", "wc -l src", "jq .x f.json", "du -sh target"].iter().enumerate() {
        rows.extend(repeated(&format!("s{index}"), 1 + index as u32, noise));
    }
    rows.extend(repeated("stale", 1, "ls").into_iter().map(|mut value| {
        value["ts"] = json!("2026-10-01T10:00:00Z");
        value
    }));
    rows.push(row("partial", 1, "2026-11-04T09:00:00Z", "command=cargo check && git add . && git status"));
    rows.into_iter().map(|value| serde_json::from_value(value).expect("row")).collect()
}

fn now() -> f64 {
    crate::foundation::clock::Timestamp::parse(NOW).expect("now").unix_seconds_f64()
}

#[test]
fn a_bash_command_becomes_its_program_and_subcommand_steps() {
    assert_eq!(command_of("command=git status\ndescription=Show status"), Some("git status"));
    assert_eq!(command_of("file_path=/x"), None);
    assert_eq!(steps("cd /x && FOO=1 depbox run -- nix develop -c cargo test --locked -p north-core | tail -3"), ["cargo test", "tail"]);
    assert_eq!(steps("git -C /x status; /usr/bin/rg -n foo; echo done"), ["git", "rg"]);
}

#[test]
fn one_four_step_sequence_repeated_across_five_sessions_yields_one_box_with_count_five() {
    let found = recurring(&sessions(&fixture()), now(), &Limits::default());
    assert_eq!(found.len(), 1, "{found:?}");
    let pattern = &found[0];
    assert_eq!(pattern.steps, ["cargo check", "git add", "git commit", "git push"]);
    assert_eq!(pattern.count(), 5);
    assert_eq!(pattern.subcommand(), "north check-add-commit-push");
    let finding = pattern.finding(Some("north#112"), &Limits::default());
    assert_eq!(finding.detector, Detector::RecurringSequence);
    assert_eq!(finding.key, "sequence:cargo-check>git-add>git-commit>git-push");
    assert_eq!(finding.instances, ["s0", "s1", "s2", "s3", "s4"]);
    assert!(finding.box_line().starts_with("- [ ] **Make the recurring sequence `cargo check > git add > git commit > git push` the subcommand `north check-add-commit-push`.** 5 sessions within 7 days ran the 4-step sequence"), "{}", finding.box_line());
}

#[test]
fn a_sequence_below_the_session_floor_or_outside_the_window_files_nothing() {
    let rows = fixture();
    let fewer: Vec<Row> = rows.into_iter().filter(|row| row.session != "s4").collect();
    assert!(recurring(&sessions(&fewer), now(), &Limits::default()).is_empty());
    assert!(recurring(&sessions(&fixture()), now() + 30.0 * 86_400.0, &Limits::default()).is_empty());
}
