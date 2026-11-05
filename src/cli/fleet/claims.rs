//! The `north threads` adapter behind the fleet's claims: a session holds its task in the ledger dispatch reads.
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::platform::process;
use crate::fleet::claims::{Claims, Rows};
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

/// A launch asks the ledger only about the items it names, through [`Fleet::release_exited_claims_on`], so parallel launches never each read every claim.
const SCOPED_VERBS: [&str; 1] = ["launch"];

/// Clears what exited sessions left in the ledger before a mutating verb, so no such command reports a claim an exited owner still appears to hold.
pub(super) fn release_exited_claims(fleet: &Fleet, arguments: &[String]) {
    if arguments.first().is_some_and(|verb| READ_VERBS.contains(&verb.as_str()) || SCOPED_VERBS.contains(&verb.as_str())) {
        return;
    }
    if let Err(error) = fleet.release_exited_claims() {
        eprintln!("north fleet: exited sessions' claims not cleared: {}", error.user_message());
    }
}

/// How long a fleet verb waits on one threads ledger call, because the ledger can sit across ssh behind a hung courier and a verb must not wait on it.
const LEDGER_LIMIT: Duration = Duration::from_secs(2);

pub struct ThreadsCli {
    program: Option<PathBuf>,
    limit: Duration,
}

impl ThreadsCli {
    pub fn new() -> Self {
        Self { program: None, limit: LEDGER_LIMIT }
    }

    fn threads(&self, args: &[&str]) -> NorthResult<String> {
        let program = match &self.program {
            Some(program) => program.clone(),
            None => running_north()?,
        };
        let mut command = Command::new(program);
        command.arg("threads").args(args);
        let Some(output) = process::run_group_within(command, self.limit) else {
            return Err(NorthError::State(format!("the threads ledger did not answer `threads {}` within {} s", args.join(" "), self.limit.as_secs())));
        };
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(NorthError::Refused(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
    }
}

impl Claims for ThreadsCli {
    fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
        match self.threads(&["claim", item, "--by", session]) {
            Err(NorthError::Refused(why)) if !why.contains(OWNED) => Err(NorthError::State(why)),
            other => other.map(|said| moved(said.trim(), item, session)),
        }
    }

    fn release(&self, item: &str, session: &str) -> NorthResult<()> {
        self.threads(&["release", &canonical(item), "--by", session, "--held-by", session]).map(drop)
    }

    fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
        let words: Vec<String> = match rows {
            Rows::Items(items) => items.iter().map(|item| canonical(item)).collect(),
            Rows::Owners(owners) => owners.iter().flat_map(|owner| ["--owner".to_owned(), owner.clone()]).collect(),
        };
        let args: Vec<&str> = std::iter::once("holder").chain(words.iter().map(String::as_str)).collect();
        let rows: Value = serde_json::from_str(&self.threads(&args)?)?;
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
            assert_eq!(claims.holders(Rows::Items(&words(&["north#1"]))).unwrap(), [("north#1".to_owned(), "gone".to_owned())], "{verb} swept claims");
        }
        release_exited_claims(&fleet, &words(&["launch", "--task", "north#1"]));
        assert_eq!(claims.holders(Rows::Items(&words(&["north#1"]))).unwrap(), [("north#1".to_owned(), "gone".to_owned())], "launch swept every claim");
        release_exited_claims(&fleet, &words(&["waits"]));
        assert!(claims.holders(Rows::Items(&words(&["north#1"]))).unwrap().is_empty());
    }

    struct Counted(Held, std::sync::atomic::AtomicUsize);

    impl Claims for Counted {
        fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
            self.0.hold(item, session)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.0.release(item, session)
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.0.holders(rows)
        }
    }

    #[test]
    fn an_exited_session_is_swept_once_and_later_verbs_never_ask_the_ledger_about_it() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let claims = Arc::new(Counted(Held::default(), Default::default()));
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, claims.clone()).unwrap();
        for (name, item) in [("gone", "north#1"), ("left", "north#2")] {
            let session = home.path().join(".local/state/north/fleet").join(name);
            std::fs::create_dir_all(&session).unwrap();
            let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
            std::fs::write(session.join("session.json"), record.to_string()).unwrap();
            claims.hold(item, name).unwrap();
        }
        let reads = || claims.1.load(std::sync::atomic::Ordering::SeqCst);

        assert_eq!(fleet.release_exited_claims().unwrap(), 2);
        assert_eq!(reads(), 1);
        assert!(claims.0.holders(Rows::Items(&["north#1".to_owned(), "north#2".to_owned()])).unwrap().is_empty());
        for name in ["gone", "left"] {
            assert!(home.path().join(".local/state/north/fleet").join(name).join(crate::fleet::records::CLAIMS_RELEASED).exists(), "{name} unmarked");
        }

        for _ in 0..3 {
            release_exited_claims(&fleet, &["send".to_owned(), "gone".to_owned()]);
        }
        assert_eq!(reads(), 1, "a later send asked the ledger about sessions already swept");
    }

    struct Bounded(Held, std::sync::atomic::AtomicUsize);

    impl Claims for Bounded {
        fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
            self.0.hold(item, session)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.0.release(item, session)
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match rows {
                Rows::Owners(owners) if owners.len() > 64 => Err(NorthError::State(format!("the threads ledger did not answer about {} owners within 2 s", owners.len()))),
                rows => self.0.holders(rows),
            }
        }
    }

    #[test]
    fn six_hundred_exited_sessions_clear_in_reads_the_ledger_answers_and_a_drained_backlog_asks_nothing() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let claims = Arc::new(Bounded(Held::default(), Default::default()));
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, claims.clone()).unwrap();
        let items: Vec<String> = (1..=600).map(|n| format!("north#{n}")).collect();
        for (n, item) in items.iter().enumerate() {
            let name = format!("gone-{n}");
            let session = home.path().join(".local/state/north/fleet").join(&name);
            std::fs::create_dir_all(&session).unwrap();
            let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
            std::fs::write(session.join("session.json"), record.to_string()).unwrap();
            claims.hold(item, &name).unwrap();
        }

        let released: usize = (0..5).map(|_| fleet.release_exited_claims().expect("a sweep read more owners than the ledger answers")).sum();
        assert_eq!(released, 600);
        assert!(claims.0.holders(Rows::Items(&items)).unwrap().is_empty());
        let reads = claims.1.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(fleet.release_exited_claims().unwrap(), 0);
        assert_eq!(claims.1.load(std::sync::atomic::Ordering::SeqCst), reads, "a drained backlog still asked the ledger");
    }

    fn a_hung_courier(home: &std::path::Path) -> ThreadsCli {
        let script = home.join("hung-north");
        crate::foundation::shim::write_script(&script, "sleep 30\n");
        ThreadsCli { program: Some(script), limit: LEDGER_LIMIT }
    }

    #[test]
    fn a_mutating_verb_returns_within_three_seconds_while_the_threads_host_is_unreachable() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, Arc::new(a_hung_courier(home.path()))).unwrap();
        let session = home.path().join(".local/state/north/fleet/gone");
        std::fs::create_dir_all(&session).unwrap();
        let record = json!({"name": "gone", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
        std::fs::write(session.join("session.json"), record.to_string()).unwrap();

        let started = std::time::Instant::now();
        let swept = fleet.release_exited_claims();
        assert!(started.elapsed() < Duration::from_secs(3), "the sweep took {:?}", started.elapsed());
        let note = swept.unwrap_err().user_message();
        assert!(note.contains("the threads ledger did not answer `threads holder --owner gone` within 2 s") && !note.contains('\n'), "{note}");

        let started = std::time::Instant::now();
        crate::fleet::claims::hold_all(&a_hung_courier(home.path()), &["north#1".to_owned()], "gone").unwrap();
        assert!(started.elapsed() < Duration::from_secs(3), "the launch claim took {:?}", started.elapsed());
    }

    #[test]
    fn an_owner_qualified_task_releases_the_item_name_the_claim_was_recorded_under() {
        assert_eq!(canonical("tompassarelli/makgora#492"), "makgora#492");
        assert_eq!(canonical("makgora#492"), "makgora#492");
    }
}
