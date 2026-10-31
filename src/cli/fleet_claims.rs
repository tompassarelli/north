//! The `north threads` adapter behind the fleet's claims: a session holds its task in the ledger dispatch reads.
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::claims::Claims;
use crate::fleet::registry::Fleet;
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

/// A verb that only reads leaves the sweep to the mutating verbs and the turn watch, because the sweep asks the threads ledger, which can sit across ssh, and the hooks that call `fleet list` run on every prompt.
const READ_VERBS: [&str; 6] = ["list", "stranded", "tree", "spend", "accounts", "show"];

/// Clears what exited sessions left in the ledger before a mutating verb, so no such command reports a claim an exited owner still appears to hold.
pub(super) fn release_exited_claims(fleet: &Fleet, arguments: &[String]) {
    if arguments.first().is_some_and(|verb| READ_VERBS.contains(&verb.as_str())) {
        return;
    }
    if let Err(error) = fleet.release_exited_claims() {
        eprintln!("north fleet: exited sessions' claims not cleared: {}", error.user_message());
    }
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
        Self::threads(&["release", &canonical(item), "--by", session, "--held-by", session]).map(drop)
    }

    fn holders(&self) -> NorthResult<Vec<(String, String)>> {
        let rows: Value = serde_json::from_str(&Self::threads(&["list", "--json", "--all"])?)?;
        Ok(rows.as_array().into_iter().flatten().filter_map(|row| Some((row["item"].as_str()?.to_owned(), row["owner"].as_str()?.to_owned()))).collect())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use serde_json::json;

    use crate::fleet::claims::Held;
    use crate::fleet::runtimes::Runtimes;
    use crate::ledger::Ledger;
    use crate::providers::claude::fleet::Claude;

    #[test]
    fn reading_the_fleet_never_asks_the_claims_ledger_while_a_mutating_verb_sweeps_exited_claims() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let claims = Arc::new(Held::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, claims.clone()).unwrap();
        let session = home.path().join(".local/state/north/fleet/gone");
        std::fs::create_dir_all(&session).unwrap();
        let record = json!({"name": "gone", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
        std::fs::write(session.join("session.json"), record.to_string()).unwrap();
        claims.hold("north#1", "gone").unwrap();
        let words = |verb: &[&str]| verb.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();

        for verb in ["list", "stranded", "tree", "spend", "accounts", "show"] {
            release_exited_claims(&fleet, &words(&[verb]));
            assert_eq!(claims.holders().unwrap(), [("north#1".to_owned(), "gone".to_owned())], "{verb} swept claims");
        }
        release_exited_claims(&fleet, &words(&["waits"]));
        assert!(claims.holders().unwrap().is_empty());
    }

    #[test]
    fn an_owner_qualified_task_releases_the_item_name_the_claim_was_recorded_under() {
        assert_eq!(canonical("tompassarelli/smashcraft#492"), "smashcraft#492");
        assert_eq!(canonical("smashcraft#492"), "smashcraft#492");
    }
}
