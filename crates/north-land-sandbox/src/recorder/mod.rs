mod arch;
mod calls;
mod filter;
mod memory;
mod trace;

use std::collections::BTreeSet;
use std::path::PathBuf;

use north_base::verdict_cache::path_set::{Entry, Kind};

use crate::classify::{Classifier, Outcome, Role, Roots, TARGET_LABEL};

pub use filter::install;
pub use trace::follow;

pub struct Report {
    pub entries: Vec<Entry>,
    pub refused: Vec<String>,
}

pub struct Recorder {
    classifier: Classifier,
    entries: BTreeSet<Entry>,
    refused: BTreeSet<String>,
    allowed: Option<BTreeSet<Entry>>,
}

impl Recorder {
    pub fn new(roots: Roots, allowed: Option<Vec<Entry>>) -> Self {
        Self { classifier: Classifier::new(roots), entries: BTreeSet::new(), refused: BTreeSet::new(), allowed: allowed.map(|entries| entries.into_iter().collect()) }
    }

    pub fn observe(&mut self, pid: u32, role: Role, path: PathBuf) {
        let outcome = self.classifier.classify(pid, role, &path);
        if let Some(allowed) = &self.allowed
            && role != Role::Probe
            && refuses(allowed, &outcome)
        {
            self.refused.insert(path.display().to_string());
        }
        match outcome {
            Outcome::Drop => {}
            Outcome::Record(entry) => {
                self.entries.insert(entry);
            }
            Outcome::Unreusable { reason, .. } => self.mark_unreusable(reason),
        }
    }

    pub fn mark_unreusable(&mut self, reason: String) {
        self.entries.insert(Entry { kind: Kind::Unreusable, path: reason.replace(['\t', '\n'], " ") });
    }

    pub fn finish(self) -> Report {
        Report { entries: self.entries.into_iter().collect(), refused: self.refused.into_iter().collect() }
    }
}

fn refuses(allowed: &BTreeSet<Entry>, outcome: &Outcome) -> bool {
    match outcome {
        Outcome::Record(entry) => entry.kind != Kind::Absent && !entry.path.starts_with(TARGET_LABEL) && !allowed.contains(entry),
        Outcome::Unreusable { refused, .. } => *refused,
        Outcome::Drop => false,
    }
}
