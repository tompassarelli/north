use std::path::{Path, PathBuf};

use north_coverage::map::{self, Map};
use north_coverage::run_selected::repository;

use crate::foundation::clock::Timestamp;

use super::{Conclusion, Run, Source, Window};

pub fn default_path(repo: &Path) -> PathBuf {
    map::path_for(&repository(repo))
}

pub fn run_in(path: &Path, window: &Window, named: bool) -> Result<Option<Run>, String> {
    if !named && !path.exists() {
        return Ok(None);
    }
    Ok(in_window(Map::load(path)?, window))
}

fn in_window(map: Map, window: &Window) -> Option<Run> {
    let at = |text: &str| Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds);
    let recorded = i64::try_from(map.recorded).ok()?;
    let after = at(&window.since).is_some_and(|since| recorded >= since);
    let before = window.until.as_deref().is_none_or(|until| at(until).is_some_and(|until| recorded < until));
    if !(after && before) {
        return None;
    }
    let mut failed: Vec<String> = map.failed.into_iter().map(|test| test.name).collect();
    failed.sort();
    failed.dedup();
    let conclusion = if failed.is_empty() { Conclusion::Success } else { Conclusion::Failure };
    Some(Run { source: Source::Coverage(failed), sha: map.commit, conclusion, created: Timestamp::from_unix_seconds(recorded).iso() })
}
