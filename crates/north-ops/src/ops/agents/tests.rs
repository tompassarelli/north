use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use super::busy::{Roots, working};
use crate::foundation::clock;

fn transcript(path: &Path, age_minutes: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::create(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(age_minutes * 60))
        .unwrap();
}

fn fixture() -> (TempDir, Roots) {
    let dir = tempfile::tempdir().unwrap();
    let roots = Roots {
        codex: dir.path().join("codex"),
        claude: dir.path().join("claude"),
    };
    fs::create_dir_all(roots.claude.join("-home-tom")).unwrap();
    (dir, roots)
}

fn busy(roots: &Roots, window: u64) -> bool {
    working(roots, clock::local_today(), window, SystemTime::now())
}

fn codex_today(roots: &Roots, name: &str) -> std::path::PathBuf {
    let today = clock::local_today();
    roots
        .codex
        .join(format!(
            "{:04}/{:02}/{:02}",
            today.year(),
            u8::from(today.month()),
            today.day()
        ))
        .join(name)
}

type Case = (&'static str, &'static [(&'static str, u64)], u64, bool);

#[test]
fn a_session_is_busy_only_while_a_today_or_yesterday_transcript_was_written_inside_the_window() {
    let yesterday = clock::local_today().previous_day().unwrap();
    let yesterday = format!("{:04}/{:02}/{:02}", yesterday.year(), u8::from(yesterday.month()), yesterday.day());
    let cases: [Case; 7] = [
        ("no transcripts", &[], 5, false),
        ("stale transcripts", &[("codex-today/rollout-old.jsonl", 20), ("claude/-home-tom/old.jsonl", 20)], 5, false),
        ("live codex", &[("codex-today/rollout-live.jsonl", 0)], 5, true),
        ("live claude", &[("claude/-home-tom/live.jsonl", 0)], 5, true),
        ("zero window", &[("claude/-home-tom/live.jsonl", 0)], 0, false),
        ("codex past midnight", &[("codex-yesterday/rollout-late.jsonl", 0)], 5, true),
        ("non-transcripts", &[("claude/-home-tom/notes.txt", 0), ("claude/top-level.jsonl", 0)], 5, false),
    ];
    for (name, files, window, expected) in cases {
        let (_dir, roots) = fixture();
        for (file, age) in files {
            let path = match file.split_once('/').unwrap() {
                ("codex-today", rest) => codex_today(&roots, rest),
                ("codex-yesterday", rest) => roots.codex.join(&yesterday).join(rest),
                (_, rest) => roots.claude.join(rest),
            };
            transcript(&path, *age);
        }
        assert_eq!(busy(&roots, window), expected, "{name}");
    }
}
