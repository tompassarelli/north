use super::{Detector, Finding};
use north_foundation::foundation::clock::Timestamp;

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
