use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use super::Verb;
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

#[test]
fn no_transcripts_is_idle() {
    let (_dir, roots) = fixture();
    assert!(!busy(&roots, 5));
}

#[test]
fn only_stale_transcripts_are_idle() {
    let (_dir, roots) = fixture();
    transcript(&codex_today(&roots, "rollout-old.jsonl"), 20);
    transcript(&roots.claude.join("-home-tom/old.jsonl"), 20);
    assert!(!busy(&roots, 5));
}

#[test]
fn a_live_codex_session_is_busy() {
    let (_dir, roots) = fixture();
    transcript(&codex_today(&roots, "rollout-live.jsonl"), 0);
    assert!(busy(&roots, 5));
}

#[test]
fn a_live_claude_session_is_busy() {
    let (_dir, roots) = fixture();
    transcript(&roots.claude.join("-home-tom/live.jsonl"), 0);
    assert!(busy(&roots, 5));
}

#[test]
fn a_zero_window_is_never_busy() {
    let (_dir, roots) = fixture();
    transcript(&roots.claude.join("-home-tom/live.jsonl"), 0);
    assert!(!busy(&roots, 0));
}

#[test]
fn yesterdays_folder_covers_sessions_past_midnight() {
    let (_dir, roots) = fixture();
    let yesterday = clock::local_today().previous_day().unwrap();
    let folder = format!(
        "{:04}/{:02}/{:02}",
        yesterday.year(),
        u8::from(yesterday.month()),
        yesterday.day()
    );
    transcript(&roots.codex.join(folder).join("rollout-late.jsonl"), 0);
    assert!(busy(&roots, 5));
}

#[test]
fn non_transcript_files_do_not_count() {
    let (_dir, roots) = fixture();
    transcript(&roots.claude.join("-home-tom/notes.txt"), 0);
    transcript(&roots.claude.join("top-level.jsonl"), 0);
    assert!(!busy(&roots, 5));
}

#[test]
fn verbs_route_to_their_owner() {
    assert_eq!(Verb::named("status"), Some(Verb::Catalog));
    assert_eq!(Verb::named("sync"), Some(Verb::Sync));
    assert_eq!(Verb::named("usage"), Some(Verb::Script("usage.py")));
    assert_eq!(Verb::named("dispatch"), None);
}
