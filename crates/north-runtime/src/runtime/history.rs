//! The problems each host showed while it ran each runtime, so settle can tell a fault a landing brought from one the
//! runtime before it already had.
use super::layout::Layout;
use super::probe;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const RUNTIMES_KEPT: usize = 20;

#[derive(Deserialize, Serialize)]
struct Seen {
    runtime: String,
    problems: Vec<String>,
}

pub(super) struct History {
    path: PathBuf,
    hosts: BTreeMap<String, Vec<Seen>>,
}

impl History {
    pub(super) fn load(layout: &Layout) -> Self {
        let path = layout.state.join("runtime-settle-problems.json");
        let hosts = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { path, hosts }
    }

    pub(super) fn note(&mut self, host: &str, runtime: &str, problems: &[String]) {
        let problems = problems.iter().filter(|problem| !problem.starts_with(probe::LAGS)).cloned().collect();
        let seen = self.hosts.entry(host.to_owned()).or_default();
        match seen.iter_mut().find(|seen| seen.runtime == runtime) {
            Some(known) => known.problems = problems,
            None => seen.push(Seen { runtime: runtime.to_owned(), problems }),
        }
        seen.drain(..seen.len().saturating_sub(RUNTIMES_KEPT));
    }

    pub(super) fn before(&self, host: &str, landing: &str, precedes: impl Fn(&str) -> bool) -> &[String] {
        let seen = self.hosts.get(host).map(Vec::as_slice).unwrap_or_default();
        seen.iter().rev().find(|seen| seen.runtime != landing && precedes(&seen.runtime)).map_or(&[], |seen| &seen.problems)
    }

    pub(super) fn save(&self) -> std::io::Result<()> {
        fs::write(&self.path, serde_json::to_string(&self.hosts)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Layout};

    #[test]
    fn a_landing_inherits_the_problems_of_the_nearest_earlier_runtime_and_not_its_own() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_owned() };
        let mut history = History::load(&layout);
        history.note("nexus", "aaaa", &["unit x is failed".to_owned()]);
        history.note("nexus", "bbbb", &["unit y is failed".to_owned()]);
        history.note("nexus", "cccc", &["unit z is failed".to_owned()]);
        history.save().unwrap();
        let history = History::load(&layout);
        let earlier = |runtime: &str| runtime < "cccc";
        assert_eq!(history.before("nexus", "cccc", earlier), ["unit y is failed".to_owned()]);
        assert!(history.before("laptop", "cccc", earlier).is_empty());
    }
}
