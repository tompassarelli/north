use std::path::{Path, PathBuf};

use super::candidate::git;
use super::queue::Ticket;

pub const TRAILER: &str = "Lander-fix:";

pub const PATHS: &[&str] = &["crates/north-land", "crates/north-safe-push/", "crates/north-check-environment/", "crates/north-offload/src/land_offload/", "tests/it/land", "docs/features/04-landing-and-lanes/", "docs/features.md", "docs/rationale/land"];

pub fn reason(bodies: &str) -> Option<String> {
    bodies.lines().find_map(|line| line.strip_prefix(TRAILER)).map(str::trim).filter(|reason| !reason.is_empty()).map(str::to_owned)
}

pub fn outside<'a>(files: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    files.into_iter().filter(|file| !PATHS.iter().any(|path| file.starts_with(path))).collect()
}

pub fn marked(root: &Path, base: &str, head: &str) -> Result<Option<String>, String> {
    let Some(reason) = reason(&git(root, &["log", "--format=%B", &format!("{base}..{head}")])?) else {
        return Ok(None);
    };
    let files = git(root, &["diff", "--no-renames", "--name-only", &format!("{base}...{head}")])?;
    match outside(files.lines()).as_slice() {
        [] => Ok(Some(reason)),
        others => Err(format!("a {TRAILER} lane changes only lander and landing code ({}), and this lane also changes {}; drop the trailer or move those files to another lane", PATHS.join(", "), others.join(", "))),
    }
}

pub fn repairs(tickets: &[(PathBuf, Ticket)]) -> Vec<(PathBuf, Ticket)> {
    tickets.iter().filter(|(_, ticket)| ticket.lander_fix.is_some() || ticket.red_fix.is_some()).cloned().collect()
}
