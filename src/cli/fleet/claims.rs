//! The `north threads` adapter behind the fleet's claims: a session holds its task in the ledger dispatch reads.
use std::fs::{File, OpenOptions, TryLockError};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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

/// A verb that only reads leaves the sweep to the mutating verbs, because the hooks that call `fleet list` run on every prompt.
const READ_VERBS: [&str; 6] = ["list", "stranded", "tree", "spend", "accounts", "show"];

/// A launch asks the ledger only about the items it names, through [`Fleet::release_exited_claims_on`], so parallel launches never each read every claim.
const SCOPED_VERBS: [&str; 1] = ["launch"];

/// The verb a mutating verb starts in the background to clear exited sessions' claims.
pub(super) const SWEEP_VERB: &str = "sweep-claims";

/// Whether `arguments` name a verb that starts the background claims sweep.
fn sweeps(arguments: &[String]) -> bool {
    !arguments.first().is_some_and(|verb| READ_VERBS.contains(&verb.as_str()) || SCOPED_VERBS.contains(&verb.as_str()) || verb == SWEEP_VERB)
}

/// Starts the claims sweep in the background when a mutating verb finds exited sessions no sweep has cleared, so no verb waits on the threads ledger, which sits across ssh on a host that can be loaded, or reports its slowness.
pub(super) fn release_exited_claims(fleet: &Fleet, arguments: &[String]) {
    if !sweeps(arguments) {
        return;
    }
    let started = fleet.unswept().and_then(|unswept| if unswept.is_empty() { Ok(()) } else { start_sweep(fleet) });
    if let Err(error) = started {
        eprintln!("north fleet: exited sessions' claims sweep not started: {}", error.user_message());
    }
}

fn start_sweep(fleet: &Fleet) -> NorthResult<()> {
    let Some(log) = sweep_log_unless_running(fleet)? else {
        return Ok(());
    };
    log.unlock()?;
    let mut command = Command::new(running_north()?);
    command.args(["fleet", SWEEP_VERB]).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log).process_group(0);
    let mut child = command.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Clears every exited session's claims, one sweep per fleet at a time: a sweep that finds another holding the log leaves the backlog to it, and the one that holds it writes only its own outcome there.
pub(super) fn sweep(fleet: &Fleet) -> NorthResult<()> {
    let Some(log) = sweep_log_unless_running(fleet)? else {
        return Ok(());
    };
    log.set_len(0)?;
    println!("released {} claims exited sessions held", fleet.release_exited_claims()?);
    Ok(())
}

/// The sweep log, opened for appending and locked by this process, or `None` while a running sweep holds it.
fn sweep_log_unless_running(fleet: &Fleet) -> NorthResult<Option<File>> {
    let log = OpenOptions::new().create(true).append(true).open(fleet.claims_sweep_log())?;
    match log.try_lock() {
        Ok(()) => Ok(Some(log)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

/// How long a fleet verb waits on one threads ledger call, because the ledger can sit across ssh behind a hung courier and a verb must not wait on it.
const LEDGER_LIMIT: Duration = Duration::from_secs(2);

/// How long the background claims sweep waits on one threads ledger call: no verb waits on it, so it outlasts the threads client's own 20 s ssh bound.
const SWEEP_LIMIT: Duration = Duration::from_secs(30);

pub struct ThreadsCli {
    program: Option<PathBuf>,
    limit: Duration,
}

impl ThreadsCli {
    pub fn new() -> Self {
        Self { program: None, limit: LEDGER_LIMIT }
    }

    /// The ledger adapter for the verb `arguments` name: the background claims sweep waits longer on each call than a verb someone is waiting on.
    pub fn for_verb(arguments: &[String]) -> Self {
        let limit = if arguments.first().is_some_and(|verb| verb == SWEEP_VERB) { SWEEP_LIMIT } else { LEDGER_LIMIT };
        Self { program: None, limit }
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
    fn reading_the_fleet_launching_and_the_sweep_itself_never_start_the_claims_sweep_while_every_other_verb_does() {
        let words = |verb: &[&str]| verb.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        for verb in ["list", "stranded", "tree", "spend", "accounts", "show", "launch", SWEEP_VERB] {
            assert!(!sweeps(&words(&[verb, "x"])), "{verb} started the sweep");
        }
        for verb in ["send", "forget", "tick", "waits"] {
            assert!(sweeps(&words(&[verb, "x"])), "{verb} left exited claims unswept");
        }
        assert_eq!(ThreadsCli::for_verb(&words(&[SWEEP_VERB])).limit, SWEEP_LIMIT);
        assert_eq!(ThreadsCli::for_verb(&words(&["send", "x"])).limit, LEDGER_LIMIT);
    }

    #[test]
    fn a_sweep_that_finds_another_holding_the_log_asks_the_ledger_nothing_and_leaves_its_outcome() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let claims = Arc::new(Counted(Held::default(), Default::default()));
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, claims.clone()).unwrap();
        let session = home.path().join(".local/state/north/fleet/gone");
        std::fs::create_dir_all(&session).unwrap();
        let record = json!({"name": "gone", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
        std::fs::write(session.join("session.json"), record.to_string()).unwrap();
        claims.hold("north#1", "gone").unwrap();
        let running = OpenOptions::new().create(true).append(true).open(fleet.claims_sweep_log()).unwrap();
        std::io::Write::write_all(&mut &running, b"released 3 claims exited sessions held\n").unwrap();
        running.lock().unwrap();

        sweep(&fleet).unwrap();
        assert_eq!(claims.1.load(std::sync::atomic::Ordering::SeqCst), 0, "a second sweep asked the ledger");
        assert_eq!(std::fs::read_to_string(fleet.claims_sweep_log()).unwrap(), "released 3 claims exited sessions held\n");
        drop(running);

        sweep(&fleet).unwrap();
        assert!(claims.0.holders(Rows::Items(&["north#1".to_owned()])).unwrap().is_empty());
        assert_eq!(fleet.unswept().unwrap(), Vec::<String>::new());
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
    fn an_exited_session_is_swept_once_and_later_sweeps_never_ask_the_ledger_about_it() {
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

        assert_eq!(fleet.unswept().unwrap(), Vec::<String>::new(), "a later verb would start a sweep for sessions already swept");
        assert_eq!(fleet.release_exited_claims().unwrap(), 0);
        assert_eq!(reads(), 1, "a later sweep asked the ledger about sessions already swept");
    }

    struct Reviving(Held, std::path::PathBuf);

    impl Claims for Reviving {
        fn hold(&self, item: &str, session: &str) -> NorthResult<Option<String>> {
            self.0.hold(item, session)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.0.release(item, session)
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            let record = json!({"name": "gone", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "stopped": false});
            std::fs::write(self.1.join("session.json"), record.to_string()).unwrap();
            self.0.holders(rows)
        }
    }

    #[test]
    fn a_session_revived_while_the_sweep_reads_the_ledger_stays_unmarked_so_its_next_exit_is_swept() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(Ledger::open(&home.path().join("ledger.db")).unwrap());
        let session = home.path().join(".local/state/north/fleet/gone");
        std::fs::create_dir_all(&session).unwrap();
        let record = json!({"name": "gone", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "stopped": true});
        std::fs::write(session.join("session.json"), record.to_string()).unwrap();
        let claims = Arc::new(Reviving(Held::default(), session.clone()));
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), ledger, claims.clone()).unwrap();
        claims.hold("north#1", "gone").unwrap();

        fleet.release_exited_claims().unwrap();
        assert!(!session.join(crate::fleet::records::CLAIMS_RELEASED).exists(), "a revived session was marked swept");
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
    fn six_hundred_exited_sessions_clear_in_one_sweep_of_reads_the_ledger_answers_and_a_drained_backlog_asks_nothing() {
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

        assert_eq!(fleet.release_exited_claims().expect("a sweep read more owners than the ledger answers"), 600);
        assert!(claims.0.holders(Rows::Items(&items)).unwrap().is_empty());
        let reads = claims.1.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(reads, 10, "600 owners in reads of 64");
        assert_eq!(fleet.release_exited_claims().unwrap(), 0);
        assert_eq!(claims.1.load(std::sync::atomic::Ordering::SeqCst), reads, "a drained backlog still asked the ledger");
    }

    fn a_hung_courier(home: &std::path::Path) -> ThreadsCli {
        let script = home.join("hung-north");
        crate::foundation::shim::write_script(&script, "sleep 30\n");
        ThreadsCli { program: Some(script), limit: LEDGER_LIMIT }
    }

    #[test]
    fn a_ledger_call_gives_up_within_three_seconds_naming_its_read_while_the_threads_host_is_unreachable() {
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
