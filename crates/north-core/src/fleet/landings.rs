use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::NorthResult;
use crate::platform::process::run_within;

use super::books::Books;
use super::model::{Machine, SessionRecord};
use super::runtime::binary;

const REMOTE_LOG: &str = r#"cat "${XDG_STATE_HOME:-$HOME/.local/state}/safe-push/pushes.tsv""#;
const REMOTE_WAIT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Push {
    pub at_ms: u64,
    pub repository: String,
    pub branch: String,
    pub oid: String,
    pub landed: String,
    pub session: String,
    pub reviewer: String,
}

pub fn observe_among(books: &dyn Books, record: &SessionRecord, pushes: &[Push]) -> NorthResult<Option<String>> {
    let Some(push) = landing(pushes, record) else { return Ok(None) };
    let task_ref = record.task_ref.as_deref().unwrap_or(&record.name);
    let recorded = books.record_landing(task_ref, &push.landed, push.at_ms, Some(push.at_ms.saturating_sub(record.created_ms)), Some(push.reviewer.as_str()).filter(|reviewer| !reviewer.is_empty()))?;
    Ok(recorded.then(|| push.landed.clone()))
}

pub fn landed_between(record: &SessionRecord, pushes: &[Push], from_ms: u64, to_ms: u64) -> Option<String> {
    landings_by(pushes, &record.name, from_ms, to_ms).next_back().map(|push| push.landed.clone())
}

/// The pushes safe-push logged for a session: this machine's log, plus, for a session whose CLI runs on another machine, that machine's log, where its landings are written.
pub struct PushLog {
    here: Vec<Push>,
    there: BTreeMap<String, Vec<Push>>,
}

impl PushLog {
    pub fn read(log: &Path) -> Self {
        Self { here: pushes(log), there: BTreeMap::new() }
    }

    pub fn of(&mut self, record: &SessionRecord) -> Vec<Push> {
        let Some(machine) = &record.machine else { return self.here.clone() };
        let remote = self.there.entry(machine.name.clone()).or_insert_with(|| remote_pushes(&binary("ssh"), machine));
        self.here.iter().chain(remote.iter()).cloned().collect()
    }
}

pub fn pushes_of(record: &SessionRecord, push_log: &Path) -> Vec<Push> {
    PushLog::read(push_log).of(record)
}

pub fn remote_pushes(ssh: &Path, machine: &Machine) -> Vec<Push> {
    let argv = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.ssh.as_str(), "--", REMOTE_LOG];
    match run_within(&ssh.to_string_lossy(), &argv, REMOTE_WAIT) {
        Some(output) if output.status.success() => parse(&String::from_utf8_lossy(&output.stdout)),
        Some(output) => {
            eprintln!("north ledger: pushes of {}: {}: {}", machine.name, output.status, String::from_utf8_lossy(&output.stderr).trim());
            Vec::new()
        }
        None => {
            eprintln!("north ledger: pushes of {}: no answer within {}s", machine.name, REMOTE_WAIT.as_secs());
            Vec::new()
        }
    }
}

pub fn landings_by<'a>(pushes: &'a [Push], session: &'a str, from_ms: u64, to_ms: u64) -> impl DoubleEndedIterator<Item = &'a Push> + 'a {
    pushes.iter().filter(move |push| push.session == session && !push.landed.is_empty() && push.at_ms + 1_000 > from_ms && push.at_ms <= to_ms)
}

pub fn push_log() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    state.unwrap_or_else(home).join("safe-push/pushes.tsv")
}

fn landing<'a>(pushes: &'a [Push], record: &SessionRecord) -> Option<&'a Push> {
    pushes.iter().rev().find(|push| push.session == record.name && !push.landed.is_empty() && push.at_ms + 1_000 > record.created_ms)
}

pub fn pushes(log: &Path) -> Vec<Push> {
    parse(&std::fs::read_to_string(log).unwrap_or_default())
}

fn parse(text: &str) -> Vec<Push> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let at_secs: u64 = fields.first()?.parse().ok()?;
            let field = |index: usize| fields.get(index).copied().unwrap_or_default().to_owned();
            Some(Push { at_ms: at_secs * 1_000, repository: field(1), branch: field(2), oid: field(3), landed: field(4), session: field(5), reviewer: field(6) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::fleet::books::{Landing, SessionSpend};

    use super::*;

    fn observe(books: &dyn Books, record: &SessionRecord, push_log: &Path) -> NorthResult<Option<String>> {
        observe_among(books, record, &pushes(push_log))
    }

    type Landed = (String, String, u64, Option<String>);

    #[derive(Default)]
    struct Landings(Mutex<Vec<Landed>>);

    impl Books for Landings {
        fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
            Ok(Vec::new())
        }

        fn landed_since(&self, _since_ms: u64) -> NorthResult<Vec<Landing>> {
            Ok(Vec::new())
        }

        fn record_landing(&self, task_ref: &str, sha: &str, landed_ms: u64, _wall_ms: Option<u64>, review: Option<&str>) -> NorthResult<bool> {
            let mut landed = self.0.lock().unwrap();
            let new = !landed.iter().any(|(task, recorded, ..)| task == task_ref && recorded == sha);
            if new {
                landed.push((task_ref.to_owned(), sha.to_owned(), landed_ms, review.map(str::to_owned)));
            }
            Ok(new)
        }
    }

    fn session(name: &str, task_ref: Option<&str>, created_ms: u64) -> SessionRecord {
        serde_json::from_value(serde_json::json!({
            "name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null,
            "created_ms": created_ms, "host_pid": null, "conversation": null, "task_ref": task_ref
        }))
        .unwrap()
    }

    #[test]
    fn a_session_lands_with_the_trunk_commit_safe_push_logged_under_its_name() {
        let log = tempfile::NamedTempFile::new().unwrap();
        let rows = [
            "1000\tnorth\tfix\told-four-column-row",
            "1001\tnorth\tfix\tlane-sha\t\tfixer",
            "1002\tnorth\tfix\tlane-sha\tlanded-by-other\tother",
            "1003\tnorth\tfix\tlane-sha\tlanded-before-start\tfixer",
            "2000\tnorth\tfix\tlane-sha\tlanded-on-main\tfixer\treviewer-one",
        ];
        std::fs::write(log.path(), rows.join("\n")).unwrap();
        let books = Landings::default();
        let fixer = session("fixer", Some("north#76"), 1_500_000);
        assert_eq!(observe(&books, &fixer, log.path()).unwrap(), Some("landed-on-main".into()));
        assert_eq!(observe(&books, &fixer, log.path()).unwrap(), None, "a landing already recorded is not recorded again");
        assert_eq!(observe(&books, &session("idle", None, 0), log.path()).unwrap(), None);
        assert_eq!(landed_between(&fixer, &pushes(log.path()), 1_999_500, 2_400_000), Some("landed-on-main".into()), "a landing in the turn's window is the turn's");
        assert_eq!(landed_between(&fixer, &pushes(log.path()), 2_001_000, 2_400_000), None);
        assert_eq!(observe(&books, &session("other", None, 0), log.path()).unwrap(), Some("landed-by-other".into()), "a session without --task lands under its name");
        assert_eq!(
            books.0.into_inner().unwrap(),
            [
                ("north#76".to_owned(), "landed-on-main".to_owned(), 2_000_000, Some("reviewer-one".to_owned())),
                ("other".to_owned(), "landed-by-other".to_owned(), 1_002_000, None)
            ]
        );
    }

    #[test]
    fn a_session_running_on_another_machine_lands_from_that_machines_push_log() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().unwrap();
        let remote_log = scratch.path().join("pushes.tsv");
        std::fs::write(&remote_log, "2000\tnorth\tfix\tlane-sha\tlanded-remotely\tplaced\trev\n").unwrap();
        let ssh = scratch.path().join("ssh");
        std::fs::write(&ssh, format!("#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\ncat {}\n", remote_log.display())).unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let machine = Machine { name: "far".into(), ssh: "tom@far".into() };
        let remote = remote_pushes(&ssh, &machine);
        let books = Landings::default();
        let placed = session("placed", Some("north#9"), 1_500_000);
        assert_eq!(observe_among(&books, &placed, &[]).unwrap(), None, "the local log alone holds no landing of a placed session");
        assert_eq!(observe_among(&books, &placed, &remote).unwrap(), Some("landed-remotely".into()));
        assert!(remote_pushes(&scratch.path().join("absent"), &machine).is_empty(), "an unreachable machine adds no pushes");
    }
}
