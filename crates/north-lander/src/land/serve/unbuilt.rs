use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::batch::Entry;
use crate::land::bisect;
use crate::land::lander::Lander;
use crate::land::probes::Probe;
use crate::land::queue;
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;

const UNBUILT: &str = "unbuilt";
const PULLED_AFTER: u32 = 2;
const FAILED: [&str; 2] = ["error: could not compile", "error: linking with"];
const CAUSES: [&str; 4] = ["rust-lld: error:", "error[E", "error: linking with", "error: could not compile"];

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Unbuilt {
    failed: BTreeMap<String, u32>,
}

pub(super) struct Pull {
    pub lane: usize,
    pub probe: usize,
    failed: u32,
    error: String,
}

fn failure(log: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    let found = |mark: &str| text.lines().find_map(|line| line.find(mark).map(|at| line[at..].trim().to_owned()));
    FAILED.into_iter().find_map(found)?;
    CAUSES.into_iter().find_map(found)
}

impl Unbuilt {
    pub(super) fn load(queue: &Path, ids: &[String]) -> Self {
        let mut stored: Self = std::fs::read_to_string(queue.join(UNBUILT)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        stored.failed.retain(|lane, _| ids.contains(lane));
        stored
    }

    pub(super) fn save(&self, queue: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        queue::write_atomic(&queue.join(UNBUILT), &text).map_err(|error| error.to_string())
    }

    pub(super) fn after(&mut self, ids: &[String], finished: &[Probe]) -> Option<Pull> {
        let failed: Vec<(usize, String)> = finished.iter().enumerate().filter(|(_, probe)| probe.refused()).filter_map(|(at, probe)| failure(&probe.log).map(|error| (at, error))).collect();
        let built: Vec<&Probe> = finished.iter().enumerate().filter(|(at, probe)| (probe.green() || probe.refused()) && !failed.iter().any(|(failed, _)| failed == at)).map(|(_, probe)| probe).collect();
        for lane in built.iter().flat_map(|probe| &probe.lanes) {
            self.failed.remove(&ids[*lane]);
        }
        let (Some((probe, error)), false) = (failed.first(), built.is_empty()) else { return None };
        let suspects: Vec<usize> = (0..ids.len()).filter(|lane| failed.iter().all(|(at, _)| finished[*at].lanes.contains(lane)) && !built.iter().any(|probe| probe.lanes.contains(lane))).collect();
        let [lane] = suspects[..] else { return None };
        let count = self.failed.entry(ids[lane].clone()).or_default();
        *count += failed.len() as u32;
        (*count >= PULLED_AFTER).then(|| Pull { lane, probe: *probe, failed: *count, error: error.clone() })
    }
}

pub(super) fn pull(lander: &Lander, rebuilds: usize, (path, ticket): Entry, proof: &Probe, pull: &Pull, main: &str) -> Result<(), String> {
    let refusal = format!("failed to build in {} probes it rode while a probe without it built: {}", pull.failed, pull.error);
    let mut stages = Stages::start();
    stages.checked(&proof.head);
    stages.attempts = rebuilds + 1;
    println!("north land: {} failed to build in {} probes it rode while a probe without it built; pulling it from the queue", ticket.branch, pull.failed);
    records::write(lander, path, ticket, &Candidate { head: &proof.head, batch_size: proof.lanes.len(), stages: &stages }, &Decision::Refused { verdict: &refusal });
    bisect::decide(path, ticket, refusal, Some(&proof.log), main)
}

#[cfg(test)]
#[path = "unbuilt_tests.rs"]
mod tests;
