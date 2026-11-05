use serde_json::json;

use super::sequence::{Limits, Session, recurring};
use super::steps::{command_of, step};
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
        row(session, 4, &ts(3), "Bash command=git add -A\ndescription=Stage the lane"),
        row(session, 5, &ts(4), "command=git commit -m 'Land the lane'"),
        row(session, 6, &ts(5), "command=RUST_LOG=info git push origin HEAD:refs/heads/claude/land-x 2>&1"),
        row(session, 7, &ts(6), &format!("command={noise} --again")),
        row(session, 8, &ts(7), "command=head -5 a.log | rg error | head -2"),
        row(session, 9, &ts(8), "command=until systemctl is-active north; do sleep 5; done"),
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
    assert_eq!(command_of("Bash command=git status\ndescription=Show status"), Some("git status"));
    assert_eq!(command_of("Read file_path=/x"), None);
    assert_eq!(command_of("file_path=/x"), None);
    assert_eq!(step("cd /x && FOO=1 depbox run -- nix develop -c cargo test --locked -p north-core | tail -3").as_deref(), Some("cargo test"));
    assert_eq!(step("git -C /x status; /usr/bin/rg -n foo; echo done").as_deref(), Some("git"));
    assert_eq!(step("rg -n foo src | head -3 && cargo check").as_deref(), Some("cargo check"));
    assert_eq!(step("timeout 20 git fetch -q origin").as_deref(), Some("git fetch"));
    assert_eq!(step("timeout --signal=KILL 5m cargo test").as_deref(), Some("cargo test"));
    assert_eq!(step("timeout -k 5 -s KILL --foreground --preserve-status 1h env X=1 cargo check").as_deref(), Some("cargo check"));
    assert_eq!(step("sed -i s/a/b/ f").as_deref(), Some("sed"));
    assert_eq!(step("ssh gex45 uptime").as_deref(), Some("ssh"));
    for filter in ["head -5 a | rg x | head -2", "cat f | jq .x | sort | uniq", "sed -n 1,9p f", "ls -la && wc -l f", "until git status; do sleep 5; done", "for f in a b; do cargo test; done", "cd /x && echo hi"] {
        assert_eq!(step(filter), None, "{filter}");
    }
}

#[test]
fn one_four_step_sequence_repeated_across_five_sessions_yields_one_box_with_count_five() {
    let found = recurring(&sessions(&fixture()), now(), &Limits::default()).kept;
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
    assert!(recurring(&sessions(&fewer), now(), &Limits::default()).kept.is_empty());
    assert!(recurring(&sessions(&fixture()), now() + 30.0 * 86_400.0, &Limits::default()).kept.is_empty());
}

#[test]
fn pipelines_of_filters_and_shell_loops_run_by_the_same_sessions_yield_no_box() {
    let noise = ["command=head -5 a.log | rg error | head -2", "command=until systemctl is-active north; do sleep 5; done", "command=rg -n x src | head", "command=while true; do sleep 1; done"];
    let mut values = Vec::new();
    for index in 0..5 {
        for round in 0..3 {
            for (offset, text) in noise.iter().enumerate() {
                values.push(row(&format!("s{index}"), round * 10 + offset as i64, &format!("2026-11-0{}T10:{round}{offset}:00Z", index + 1), text));
            }
        }
    }
    let rows: Vec<Row> = values.into_iter().map(|value| serde_json::from_value(value).expect("row")).collect();
    let mined = recurring(&sessions(&rows), now(), &Limits::default());
    assert!(mined.kept.is_empty(), "{:?}", mined.kept);
    assert_eq!(mined.dropped, 0);
}

fn ran(prefix: &str, count: usize, steps: &[String]) -> Vec<Session> {
    (0..count).map(|index| Session { id: format!("{prefix}-{index}"), at: now(), steps: steps.to_vec() }).collect()
}

fn named(verbs: &[&str]) -> Vec<String> {
    verbs.iter().map(|verb| (*verb).to_owned()).collect()
}

#[test]
fn twelve_qualifying_sequences_file_only_the_top_five_by_sessions_times_steps() {
    let long: Vec<String> = (0..8).map(|step| format!("north long-{step}")).collect();
    let mut sessions = ran("long", 5, &long);
    for sequence in 1..12 {
        let steps: Vec<String> = ["start", "middle", "end"].iter().map(|part| format!("north {part}-{sequence}")).collect();
        sessions.extend(ran(&format!("q{sequence}"), 5 + sequence, &steps));
    }
    let mined = recurring(&sessions, now(), &Limits::default());
    let ranked: Vec<(usize, usize)> = mined.kept.iter().map(|pattern| (pattern.count(), pattern.steps.len())).collect();
    assert_eq!(ranked, [(16, 3), (15, 3), (14, 3), (5, 8), (13, 3)]);
    assert_eq!(mined.dropped, 7);
    let wider = recurring(&sessions, now(), &Limits { max_filed: 20, ..Limits::default() });
    assert_eq!((wider.kept.len(), wider.dropped), (12, 0));
}

#[test]
fn a_sequence_inside_a_longer_one_with_as_many_sessions_is_not_filed_separately() {
    let four = named(&["cargo check", "git add", "git commit", "git push"]);
    let mut sessions = ran("full", 5, &four);
    let only = recurring(&sessions, now(), &Limits::default());
    assert_eq!(only.kept.iter().map(|pattern| pattern.steps.clone()).collect::<Vec<_>>(), std::slice::from_ref(&four));
    sessions.extend(ran("tail", 2, &four[1..]));
    let both = recurring(&sessions, now(), &Limits::default());
    assert_eq!(both.kept.iter().map(|pattern| (pattern.count(), pattern.steps.clone())).collect::<Vec<_>>(), [(7, four[1..].to_vec()), (5, four)]);
}

#[test]
fn a_sequence_without_a_subcommanded_program_or_ssh_files_nothing() {
    let local = named(&["du", "make", "python3"]);
    assert!(recurring(&ran("local", 6, &local), now(), &Limits::default()).kept.is_empty());
    let remote = named(&["du", "ssh", "make"]);
    assert_eq!(recurring(&ran("remote", 6, &remote), now(), &Limits::default()).kept.len(), 1);
}
