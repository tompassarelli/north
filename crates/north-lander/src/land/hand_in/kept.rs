//! The files a launch keeps in a lane for its session, which a finish or hand-in call from that session reads: the proposed message (north#1239) and the brief's item, box and Done-when command (north#1367).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::enqueue::git;
use super::checkout;
use north_base::platform::brief_trailers;

const TRAILERS_FILE: &str = "north-brief-trailers";
pub const FLAG: &str = "--brief-trailers";

pub fn path(root: &Path, file: &str) -> Option<PathBuf> {
    git(root, &["rev-parse", "--path-format=absolute", "--git-path", file]).ok().map(PathBuf::from)
}

/// The fleet session this finish process runs as, by its own variable and not a proved name, since no broker serves the finish verb.
pub fn own_session() -> Option<String> {
    std::env::var(north_base::platform::session_scope::SESSION).ok().filter(|name| !name.trim().is_empty())
}

/// The text of the kept file `file` when its first line names this process's own session by its own variable, not a proved name: the file lies in the lane's git directory, which the worker's domain writes, so it yields only what the worker could write itself, and the name match only drops a file an earlier session of the lane left.
pub fn for_session(root: &Path, file: &str) -> Option<String> {
    let own = own_session()?;
    let kept = fs::read_to_string(path(root, file)?).ok()?;
    let (session, text) = kept.split_once('\n')?;
    Some(text.to_owned()).filter(|text| session == own && !text.trim().is_empty())
}

pub fn apply_trailers(root: &Path, message: String, check: Option<&str>) -> String {
    match for_session(root, TRAILERS_FILE) {
        Some(kept) => brief_trailers::apply(&message, &kept, check),
        None => message,
    }
}

pub fn keep_trailers(session: &str) -> u8 {
    let trailers = io::read_to_string(io::stdin()).unwrap_or_default();
    let Some(kept) = std::env::current_dir().ok().and_then(|directory| checkout(&directory)).and_then(|root| path(&root, TRAILERS_FILE)) else { return 1 };
    let _ = fs::remove_file(&kept);
    if trailers.trim().is_empty() || session.trim().is_empty() || session.contains('\n') || fs::write(&kept, format!("{session}\n{}", trailers.trim())).is_err() {
        return 1;
    }
    println!("kept");
    0
}
