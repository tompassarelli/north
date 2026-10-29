//! The `north threads` adapter behind the fleet's claims: a session holds its task in the ledger dispatch reads.
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::claims::Claims;
use crate::threads::catalog::CATALOG;
use crate::threads::claims::verbs::OWNED;

/// The `repo#N` form `threads claim` records, whichever spelling `--task` carried.
fn canonical(item: &str) -> String {
    CATALOG.parse_ref(item).map_or_else(|_| item.to_owned(), |(repo, number)| format!("{repo}#{number}"))
}

/// A host outlives runtime updates, and Linux then reports its replaced binary with a " (deleted)" suffix.
fn running_north() -> NorthResult<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.to_str().and_then(|path| path.strip_suffix(" (deleted)")).map_or(exe.clone(), std::path::PathBuf::from))
}

/// The ledger's line when the claim moved to `session` from an earlier owner; a fresh or renewed claim says only that `session` owns the item.
fn moved(said: &str, item: &str, session: &str) -> Option<String> {
    (said != format!("{session} owns {}", canonical(item))).then(|| said.to_owned())
}

pub struct ThreadsCli;

impl ThreadsCli {
    fn threads(args: &[&str]) -> NorthResult<String> {
        let output = Command::new(running_north()?).arg("threads").args(args).stdin(Stdio::null()).output()?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(NorthError::Refused(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
    }
}

impl Claims for ThreadsCli {
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
        match Self::threads(&["claim", item, "--by", session]) {
            Err(NorthError::Refused(why)) if !why.contains(OWNED) => Err(NorthError::State(why)),
            other => other.map(|said| moved(said.trim(), item, session)),
        }
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        let held: Value = serde_json::from_str(&Self::threads(&["list", "--json"])?)?;
        let item = canonical(item);
        let owned = held.as_array().into_iter().flatten().any(|claim| claim["item"] == item.as_str() && claim["owner"] == session);
        if owned { Self::threads(&["release", &item, "--by", session]).map(drop) } else { Ok(()) }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_qualified_task_releases_the_item_name_the_claim_was_recorded_under() {
        assert_eq!(canonical("tompassarelli/smashcraft#492"), "smashcraft#492");
        assert_eq!(canonical("smashcraft#492"), "smashcraft#492");
    }
}
