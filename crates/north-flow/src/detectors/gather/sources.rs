//! What the followed-source detector reads: south's `agents/watch.toml` and, for each source, the commits its followed
//! path gained since its last reviewed commit in a bare clone under `~/.cache/watch-review`, fetched at most once per
//! `detector_followed_source_read_minutes`.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Gatherer, default_number, default_text};
use crate::detectors::followed_source::{self, Facts, Moved, Reading, Source};
use crate::flow::write_atomic;

const READINGS: &str = "followed-source-readings.json";
const WATCH_TOML: &str = "agents/watch.toml";
const CACHE: &str = ".cache/watch-review";
const SOURCE_REPO: &str = "south";
const FETCH_LIMIT: Duration = Duration::from_secs(300);

#[derive(Deserialize, Serialize)]
struct Read {
    at: f64,
    last_commit: String,
    count: usize,
    head: String,
}

impl Gatherer<'_> {
    fn source_git(&self, clone: &Path, arguments: &[&str]) -> Result<String, String> {
        let mut argv = vec!["git"];
        argv.extend_from_slice(arguments);
        let output = self.host.output(&argv, Some(clone), None, FETCH_LIMIT).ok_or_else(|| format!("git {} did not answer", arguments.first().copied().unwrap_or_default()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(format!("git {} failed: {}", arguments.first().copied().unwrap_or_default(), String::from_utf8_lossy(&output.stderr).trim()))
        }
    }

    fn source_moved(&self, source: &Source) -> Result<Moved, String> {
        let cache = self.places.home.join(CACHE);
        fs::create_dir_all(&cache).map_err(|error| format!("{}: {error}", cache.display()))?;
        let clone = cache.join(format!("{}.git", source.id));
        if clone.is_dir() {
            self.source_git(&clone, &["remote", "set-url", "origin", &source.repo])?;
            self.source_git(&clone, &["fetch", "--quiet", "origin", "+HEAD:refs/watch/head"])?;
        } else {
            let target = clone.to_string_lossy();
            self.source_git(&cache, &["clone", "--quiet", "--bare", "--filter=blob:none", "--", &source.repo, &target])?;
            self.source_git(&clone, &["update-ref", "refs/watch/head", "HEAD"])?;
        }
        self.source_git(&clone, &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", source.last_commit)]).map_err(|_| format!("{} not found", source.last_commit))?;
        let range = format!("{}..refs/watch/head", source.last_commit);
        let mut count = vec!["rev-list", "--count", "--no-merges", range.as_str()];
        if !source.path.is_empty() {
            count.extend(["--", source.path.as_str()]);
        }
        let count = self.source_git(&clone, &count)?.parse().map_err(|_| "rev-list gave no count".to_owned())?;
        Ok(Moved { count, head: self.source_git(&clone, &["rev-parse", "--short", "refs/watch/head"])? })
    }

    fn source_read(&self, source: &Source, saved: &mut BTreeMap<String, Read>, now: f64) -> Option<Moved> {
        let wait = default_number("detector_followed_source_read_minutes") * 60.0;
        if let Some(read) = saved.get(&source.id).filter(|read| now - read.at < wait && read.last_commit == source.last_commit) {
            return Some(Moved { count: read.count, head: read.head.clone() });
        }
        let moved = self.source_moved(source).inspect_err(|why| eprintln!("detectors: followed source {}: {why}", source.id)).ok()?;
        if self.profiling {
            saved.insert(source.id.clone(), Read { at: now, last_commit: source.last_commit.clone(), count: moved.count, head: moved.head.clone() });
        }
        Some(moved)
    }

    fn save_source_reads(&self, path: &Path, saved: &BTreeMap<String, Read>) {
        let _ = fs::create_dir_all(self.places.flow());
        if let Err(error) = serde_json::to_string(saved).map_err(std::io::Error::other).and_then(|text| write_atomic(path, &text)) {
            eprintln!("detectors: could not record the followed-source readings: {error}");
        }
    }

    pub fn followed_sources(&self, now: f64) -> Facts {
        let owner = Some(default_text("detector_followed_source_owner"));
        let Ok(text) = fs::read_to_string(self.places.checkout(SOURCE_REPO).join(WATCH_TOML)) else { return Facts { owner, readings: Vec::new() } };
        let path = self.places.flow().join(READINGS);
        let mut saved: BTreeMap<String, Read> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let readings = followed_source::sources(&text).into_iter().filter_map(|source| Some(Reading { moved: self.source_read(&source, &mut saved, now)?, source })).collect();
        if self.profiling {
            self.save_source_reads(&path, &saved);
        }
        Facts { owner, readings }
    }
}
