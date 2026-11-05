use std::path::Path;
use std::process::{Command, Output};

fn north(home: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(arguments)
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("THREADS_DB", home.join("threads.db"))
        .env("THREADS_SCHEMA_READY", "1")
        .output()
        .unwrap()
}

#[test]
fn a_reviews_recorded_findings_are_read_back_per_kind_by_worker_ledger_recommend_review() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path();
    rusqlite::Connection::open(home.join("threads.db")).unwrap();
    let findings = [
        ["fact", "", "here", "must-fix", "accepted", "217"],
        ["scenario", "", "sonnet-high", "must-fix", "accepted", "13"],
        ["scenario", "", "sonnet-high", "nit", "rejected", "19"],
        ["architecture", "decomposition", "opus-high", "should-fix", "rejected", "15"],
    ];
    for [kind, lens, tier, severity, verdict, line] in findings {
        let recorded = north(home, &["threads", "review-finding", "record", "--review", "proxy-role-1", "--plan", "docs/rationale/proxy-role.md", "--line", line, "--kind", kind, "--lens", lens, "--tier", tier, "--severity", severity, "--verdict", verdict]);
        assert!(recorded.status.success(), "{}", String::from_utf8_lossy(&recorded.stderr));
    }
    let read = north(home, &["worker-ledger", "--recommend", "Category: review"]);
    let out = String::from_utf8_lossy(&read.stdout);
    assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stderr));
    let kinds: Vec<&str> = out.lines().filter(|line| !line.starts_with(' ')).skip(1).collect();
    assert_eq!(
        kinds,
        [
            "fact: findings 1  accepted 1  rejected 0  must-fix accepted 1",
            "scenario: findings 2  accepted 1  rejected 1  must-fix accepted 1",
            "architecture: findings 1  accepted 0  rejected 1  must-fix accepted 0",
        ],
        "{out}"
    );
    assert!(out.contains("  opus-high lens decomposition"), "{out}");
}
