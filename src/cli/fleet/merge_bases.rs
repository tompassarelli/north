use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::fleet::model::SessionRecord;
use crate::fleet::reaper::{self, Evidence, Unlanded, Work};
use crate::land_ticket::Lane;

const FILE: &str = "north/reaper-merge-bases.json";

#[derive(Default, Deserialize, Serialize)]
struct Checked {
    main: String,
    landed: BTreeSet<String>,
    unlanded: BTreeSet<String>,
}

pub(super) struct MergeBases<'a> {
    evidence: &'a dyn Evidence,
    main: &'a (dyn Fn(&Path) -> Option<String> + Sync),
    file: PathBuf,
    checked: Mutex<(BTreeMap<String, Checked>, BTreeSet<String>)>,
}

impl<'a> MergeBases<'a> {
    pub(super) fn new(evidence: &'a dyn Evidence, main: &'a (dyn Fn(&Path) -> Option<String> + Sync), file: PathBuf) -> Self {
        let checked = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { evidence, main, file, checked: Mutex::new((checked, BTreeSet::new())) }
    }

    pub(super) fn keep(&self) {
        let checked = self.checked.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(bytes) = serde_json::to_vec(&checked.0) else { return };
        let scratch = self.file.with_extension("json.tmp");
        if self.file.parent().is_some_and(|parent| fs::create_dir_all(parent).is_ok()) && fs::write(&scratch, bytes).is_ok() {
            let _ = fs::rename(scratch, &self.file);
        }
    }
}

impl MergeBases<'static> {
    pub(super) fn here() -> Self {
        Self::new(&reaper::Here, &origin_main, crate::watch::state_home().join(FILE))
    }
}

fn origin_main(cwd: &Path) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(cwd).args(["rev-parse", "origin/main"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|sha| !sha.is_empty())
}

impl Evidence for MergeBases<'_> {
    fn landed(&self, cwd: &Path, sha: &str) -> bool {
        let mut guard = self.checked.lock().unwrap_or_else(PoisonError::into_inner);
        let (checked, read) = &mut *guard;
        let key = cwd.display().to_string();
        let entry = checked.entry(key.clone()).or_default();
        if entry.landed.contains(sha) {
            return true;
        }
        if read.insert(key) {
            match (self.main)(cwd) {
                Some(main) if main != entry.main => {
                    entry.main = main;
                    entry.unlanded.clear();
                }
                Some(_) => {}
                None => entry.main.clear(),
            }
        }
        if entry.main.is_empty() {
            return self.evidence.landed(cwd, sha);
        }
        if entry.unlanded.contains(sha) {
            return false;
        }
        let landed = self.evidence.landed(cwd, sha);
        if landed { entry.landed.insert(sha.to_owned()) } else { entry.unlanded.insert(sha.to_owned()) };
        landed
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        self.evidence.running(records)
    }

    fn lanes(&self) -> BTreeMap<String, Vec<Lane>> {
        self.evidence.lanes()
    }

    fn lane(&self, record: &SessionRecord) -> Option<Unlanded> {
        self.evidence.lane(record)
    }
}

#[cfg(test)]
#[path = "merge_bases_tests.rs"]
mod tests;
