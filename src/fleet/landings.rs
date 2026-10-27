use std::path::{Path, PathBuf};

use crate::error::NorthResult;

use super::books::Books;
use super::model::SessionRecord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Push {
    pub at_ms: u64,
    pub landed: String,
    pub session: String,
    pub reviewer: String,
}

pub fn observe(books: &dyn Books, record: &SessionRecord, push_log: &Path) -> NorthResult<Option<String>> {
    let pushes = pushes(push_log);
    let Some(push) = landing(&pushes, record) else { return Ok(None) };
    let task_ref = record.task_ref.as_deref().unwrap_or(&record.name);
    let recorded = books.record_landing(task_ref, &push.landed, push.at_ms, Some(push.at_ms.saturating_sub(record.created_ms)), Some(push.reviewer.as_str()).filter(|reviewer| !reviewer.is_empty()))?;
    Ok(recorded.then(|| push.landed.clone()))
}

pub fn landed_between(record: &SessionRecord, push_log: &Path, from_ms: u64, to_ms: u64) -> Option<String> {
    let during = |push: &&Push| push.session == record.name && !push.landed.is_empty() && push.at_ms + 1_000 > from_ms && push.at_ms <= to_ms;
    pushes(push_log).iter().rev().find(during).map(|push| push.landed.clone())
}

pub fn push_log() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    state.unwrap_or_else(home).join("safe-push/pushes.tsv")
}

fn landing<'a>(pushes: &'a [Push], record: &SessionRecord) -> Option<&'a Push> {
    pushes.iter().rev().find(|push| push.session == record.name && !push.landed.is_empty() && push.at_ms + 1_000 > record.created_ms)
}

fn pushes(log: &Path) -> Vec<Push> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let at_secs: u64 = fields.first()?.parse().ok()?;
            let field = |index: usize| fields.get(index).copied().unwrap_or_default().to_owned();
            Some(Push { at_ms: at_secs * 1_000, landed: field(4), session: field(5), reviewer: field(6) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::fleet::books::{Landing, SessionSpend};

    use super::*;

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
        assert_eq!(landed_between(&fixer, log.path(), 1_999_500, 2_400_000), Some("landed-on-main".into()), "a landing in the turn's window is the turn's");
        assert_eq!(landed_between(&fixer, log.path(), 2_001_000, 2_400_000), None);
        assert_eq!(observe(&books, &session("other", None, 0), log.path()).unwrap(), Some("landed-by-other".into()), "a session without --task lands under its name");
        assert_eq!(
            books.0.into_inner().unwrap(),
            [
                ("north#76".to_owned(), "landed-on-main".to_owned(), 2_000_000, Some("reviewer-one".to_owned())),
                ("other".to_owned(), "landed-by-other".to_owned(), 1_002_000, None)
            ]
        );
    }
}
