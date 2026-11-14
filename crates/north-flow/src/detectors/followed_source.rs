use super::{Detector, Finding};
use crate::foundation::clock::Timestamp;

const STALE_DAYS: i64 = 30;
const BUSY_COMMITS: usize = 10;
const DAY_S: f64 = 86_400.0;
const RECORD_REPO_PREFIX: &str = "north:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cadence {
    Busy,
    Monthly,
}

/// One `[[source]]` of south's `agents/watch.toml`: where it is followed and the review it last had.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub id: String,
    pub repo: String,
    pub path: String,
    pub last_reviewed: String,
    pub last_commit: String,
    pub record: String,
    pub cadence: Cadence,
}

/// The non-merge commits a source's followed path gained since its last reviewed commit, and its head now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Moved {
    pub count: usize,
    pub head: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reading {
    pub source: Source,
    pub moved: Moved,
}

#[derive(Default)]
pub struct Facts {
    pub readings: Vec<Reading>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid(source: &Source) -> bool {
    valid_id(&source.id) && !source.repo.starts_with('-') && (7..=40).contains(&source.last_commit.len()) && source.last_commit.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// The sources of a `watch.toml` text, each as written; a source missing its review date or carrying an unusable id, repo, commit or cadence is dropped.
pub fn sources(text: &str) -> Vec<Source> {
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let Some(tables) = document.get("source").and_then(toml_edit::Item::as_array_of_tables) else { return Vec::new() };
    tables
        .iter()
        .filter_map(|table| {
            let text = |key: &str| table.get(key).and_then(toml_edit::Item::as_str).unwrap_or_default().to_owned();
            let last_reviewed = table.get("last_reviewed").and_then(toml_edit::Item::as_value).and_then(toml_edit::Value::as_datetime)?.to_string();
            let cadence = match text("cadence").as_str() {
                "" => Cadence::Busy,
                "monthly" => Cadence::Monthly,
                _ => return None,
            };
            let record = text("record");
            let source = Source { id: text("id"), repo: text("repo"), path: text("path"), last_reviewed, last_commit: text("last_commit"), record: record.strip_prefix(RECORD_REPO_PREFIX).unwrap_or(&record).to_owned(), cadence };
            valid(&source).then_some(source)
        })
        .collect()
}

pub fn is_due(source: &Source, moved: &Moved, today: i64) -> bool {
    let reviewed = Timestamp::parse_loose(&source.last_reviewed).map(Timestamp::unix_days);
    let stale = reviewed.is_some_and(|day| today - day >= STALE_DAYS) && moved.count > 0;
    stale || (source.cadence == Cadence::Busy && moved.count >= BUSY_COMMITS)
}

/// One box per source and last reviewed commit: a source due under `watching.md`, 30 days after its last review with new commits on its path or at 10 of them, is read again, and the box stays filed until a review moves its commit.
pub fn due(facts: &Facts, now: f64) -> Vec<Finding> {
    let today = (now / DAY_S) as i64;
    facts
        .readings
        .iter()
        .filter(|reading| is_due(&reading.source, &reading.moved, today))
        .map(|Reading { source, moved }| {
            let (id, last) = (&source.id, &source.last_commit);
            let path = if source.path.is_empty() { "the whole repository".to_owned() } else { format!("`{}`", source.path) };
            let start = if source.record.is_empty() {
                "No record of it is named in agents/watch.toml; start from its last_commit.".to_owned()
            } else {
                format!("Start from its record `{}` and its \"Next review: start here\" section when it has one.", source.record)
            };
            Finding {
                detector: Detector::FollowedSource,
                key: format!("followed-source:{id}:{last}"),
                owner: None,
                title: format!("Review the followed source {id}"),
                detail: format!(
                    "{} new commits on {path} of {} since {last}, reviewed {}, head {}. Run one cycle of harness/skills/agent-policy/references/watching.md for it. {start}",
                    moved.count, source.repo, source.last_reviewed, moved.head
                ),
                done_when: format!("`docs/rationale/watch/<date>-{id}.md` has landed in north with its docs/references.md entry, and south agents/watch.toml carries a `last_commit` for {id} after {last}."),
                instances: vec![source.record.clone()],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{ExitStatus, Output};
    use std::time::Duration;

    use super::*;
    use crate::detectors::tick;
    use crate::flow::{Host, Places};

    struct Sources;

    impl Host for Sources {
        fn output(&self, argv: &[&str], cwd: Option<&Path>, _: Option<&str>, _: Duration) -> Option<Output> {
            let kept = cwd.and_then(|cwd| cwd.file_name()).and_then(|name| name.to_str()).unwrap_or_default();
            let stdout = match argv {
                ["git", "rev-list", ..] if kept == "busy.git" => "12\n",
                ["git", "rev-list", ..] => "0\n",
                ["git", "rev-parse", "--short", ..] => "9f3ab21\n",
                ["git", "remote" | "fetch" | "rev-parse", ..] => "",
                _ => return None,
            };
            Some(Output { status: ExitStatus::from_raw(0), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
        }
    }

    #[test]
    fn the_dry_run_prints_a_finding_for_a_source_that_is_due_and_none_for_one_that_is_not() {
        let root = tempfile::tempdir().unwrap();
        let places = Places { home: root.path().join("home"), state: root.path().join("state"), ledger: root.path().join("threads.db"), graph: root.path().join("graph.db") };
        let south = places.checkout("south").join("agents");
        fs::create_dir_all(&south).unwrap();
        let toml = "[[source]]\nid = \"busy\"\nrepo = \"https://example.org/busy\"\npath = \"\"\nlast_reviewed = 2026-11-04\nlast_commit = \"fd268b8\"\nrecord = \"north:docs/rationale/watch/2026-11-04-busy.md\"\n\n[[source]]\nid = \"quiet\"\nrepo = \"https://example.org/quiet\"\nlast_reviewed = 2026-11-04\nlast_commit = \"4ae2317\"\n";
        fs::write(south.join("watch.toml"), toml).unwrap();
        for id in ["busy", "quiet"] {
            fs::create_dir_all(places.home.join(".cache/watch-review").join(format!("{id}.git"))).unwrap();
        }
        let lines = tick(&places, &Sources, 1_794_000_000.0, false);
        let found: Vec<&String> = lines.iter().filter(|line| line.starts_with("detectors: followed-source followed-source:")).collect();
        assert_eq!(found, ["detectors: followed-source followed-source:busy:fd268b8: Review the followed source busy"]);
    }

    fn source(cadence: Cadence) -> Source {
        Source { id: "notes".into(), repo: "https://example.org/n".into(), path: String::new(), last_reviewed: "2026-11-05".into(), last_commit: "807ccdd".into(), record: "docs/rationale/watch/2026-11-05-notes.md".into(), cadence }
    }

    #[test]
    fn a_monthly_source_waits_out_its_month_however_many_commits_arrive_and_its_box_names_its_record() {
        let moved = Moved { count: 77, head: "68060cb".into() };
        let day = |date: &str| Timestamp::parse_loose(date).map(Timestamp::unix_days).unwrap();
        assert!(!is_due(&source(Cadence::Monthly), &moved, day("2026-11-10")));
        assert!(is_due(&source(Cadence::Monthly), &moved, day("2026-12-05")));
        assert!(is_due(&source(Cadence::Busy), &moved, day("2026-11-10")));
        let found = due(&Facts { readings: vec![Reading { source: source(Cadence::Busy), moved }] }, 1_794_000_000.0);
        assert_eq!(found.len(), 1);
        assert!(found[0].owner.is_none() && found[0].detector.files_own_issue(), "a due source is filed as an issue of its own, never on a standing owner");
        assert!(found[0].detail.contains("Start from its record `docs/rationale/watch/2026-11-05-notes.md`"));
    }
}
