use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use north_landing::landing::lane_squash::Squash;

use crate::land::slots::runner;

const DIR: &str = "check-only";

#[derive(Debug)]
pub struct Checked {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub squash: Squash,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Queue { keep_lane: bool, require_check_only: bool },
    CheckOnly,
}

#[derive(Debug)]
pub enum Outcome<Q> {
    Queued(Box<Q>),
    Checked(Checked),
}

impl Mode {
    pub fn from_flags(check_only: bool, keep_lane: bool, require_check_only: bool) -> Self {
        if check_only { Self::CheckOnly } else { Self::Queue { keep_lane, require_check_only } }
    }

    pub fn records_refusal(self) -> bool {
        matches!(self, Self::Queue { .. })
    }

    pub fn verify_required(self, state_home: &Path, origin: &str, destination: &str, head: &str) -> Result<(), String> {
        if matches!(self, Self::Queue { require_check_only: true, .. }) && !passed(&store_dir(state_home), origin, destination, head) {
            return Err(format!("the lane has not passed safe-push --check-only at its current head ({head}); run safe-push --check-only first"));
        }
        Ok(())
    }
}

fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn stamp(origin: &str, destination: &str) -> String {
    format!("{origin}\n{destination}\n")
}

pub fn store_dir(state_home: &Path) -> PathBuf {
    match std::env::var_os(north_base::verdict_cache::DIR_ENV).filter(|value| !value.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(DIR),
        None => state_home.join("north/verdict-cache").join(DIR),
    }
}

pub fn passed(dir: &Path, origin: &str, destination: &str, head: &str) -> bool {
    valid_sha(head) && fs::read_to_string(dir.join(head)).is_ok_and(|recorded| recorded == stamp(origin, destination))
}

pub fn record(dir: &Path, origin: &str, destination: &str, head: &str) -> Result<(), String> {
    if !valid_sha(head) {
        return Err(format!("check-only head {head:?} is not a 40-hex commit id"));
    }
    fs::create_dir_all(dir).map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    let mut pending = tempfile::NamedTempFile::new_in(dir).map_err(|error| format!("cannot write in {}: {error}", dir.display()))?;
    pending.write_all(stamp(origin, destination).as_bytes()).map_err(|error| format!("cannot write {}: {error}", pending.path().display()))?;
    pending.persist(dir.join(head)).map(|_| ()).map_err(|error| format!("cannot persist {}: {error}", dir.join(head).display()))
}

pub(super) fn announce_checked(checked: &Checked) -> u8 {
    match &checked.squash {
        Squash::Kept(reason) => println!("safe-push: lane not squashed: {reason}"),
        Squash::Squashed { commits, head } => println!("safe-push: squashed the lane's {commits} unpublished commits as one commit {head}"),
        Squash::Unchanged => {}
    }
    println!("Done: safe-push --check-only passed for {} {} {}", checked.root.display(), checked.branch, checked.head);
    0
}

pub(super) fn announce_queued(outbox: bool, (root, branch, head): (&Path, &str, &str), path: &Path, squash: &Squash) {
    match squash {
        Squash::Kept(reason) => println!("safe-push: lane not squashed: {reason}"),
        Squash::Squashed { commits, head } => println!("safe-push: queued the lane's {commits} unpublished commits as one commit {head}"),
        Squash::Unchanged => {}
    }
    println!("Done: queued {}{} {} {}, ticket {}", if outbox { "(outbox) " } else { "" }, root.display(), branch, head, path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()));
}

pub(super) fn refusal_row(micros: u128, repo: &Path, (lane, session): (Option<&str>, Option<&str>), error: &str) -> serde_json::Value {
    let (step, first) = match runner::failing(error) {
        Some(failing) => failing,
        None => (error.split(';').next().unwrap_or(error).trim(), error.lines().next().unwrap_or(error)),
    };
    let shorten = |text: &str, bound: usize| text.chars().take(bound).collect::<String>();
    serde_json::json!({ "micros": micros, "root": repo.to_string_lossy(), "lane": lane, "session": session, "step": shorten(step, 120), "first_error": shorten(first, 300), "error": error })
}

pub(super) fn append_refusal(path: &Path, row: &serde_json::Value) {
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = io::Write::write_all(&mut file, format!("{row}\n").as_bytes());
    }
}
