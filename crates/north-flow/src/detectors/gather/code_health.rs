//! What the code-health review detector reads: each enrolled repository's `north code-health signal` after a fetch,
//! re-read at most once per `detector_code_health_read_minutes` and recorded into the review loop when filing.
use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Gatherer, default_number};
use crate::detectors::code_health_review::{self, Reading};
use crate::detectors::default_list;
use crate::flow::write_atomic;

const READINGS: &str = "code-health-readings.json";
const SIGNAL_LIMIT: Duration = Duration::from_secs(300);
const NO_MARKER: &str = "none";

#[derive(Deserialize, Serialize)]
struct Read {
    at: f64,
    since: String,
    reasons: Vec<String>,
}

fn marker(state: &std::path::Path, repo: &str) -> String {
    let text = fs::read_to_string(state.join("code-health").join(repo).join("review.json")).unwrap_or_default();
    let sha = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|value| value["sha"].as_str().map(|sha| sha.chars().take(10).collect::<String>()));
    sha.unwrap_or_else(|| NO_MARKER.to_owned())
}

impl Gatherer<'_> {
    fn code_health_repos(&self) -> Vec<String> {
        let set_aside = crate::platform::orchestration::read().map(|document| crate::platform::orchestration::set_aside_repos(&document)).unwrap_or_default();
        default_list("detector_code_health_repos").into_iter().filter(|repo| !set_aside.contains(repo)).collect()
    }

    fn code_health_signal(&self, repo: &str) -> Option<Vec<String>> {
        let checkout = self.places.checkout(repo);
        if !checkout.exists() {
            return None;
        }
        let path = checkout.to_string_lossy();
        self.host.run(&["git", "-C", &path, "fetch", "-q", "origin"], None, None)?;
        let mut argv = vec!["north", "code-health", "signal", "--repo", &path, "--rev", "origin/main"];
        if self.profiling {
            argv.push("--record");
        }
        let output = self.host.output(&argv, None, None, SIGNAL_LIMIT)?;
        match output.status.code() {
            Some(0) => Some(code_health_review::reasons(&String::from_utf8_lossy(&output.stdout))),
            Some(1) => Some(Vec::new()),
            _ => None,
        }
    }

    pub fn code_health_readings(&self, now: f64) -> code_health_review::Facts {
        let wait = default_number("detector_code_health_read_minutes") * 60.0;
        let path = self.places.flow().join(READINGS);
        let mut saved: BTreeMap<String, Read> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let told = north_graph_loops::code_health::read(&self.places.graph).unwrap_or_default();
        let mut readings = Vec::new();
        for repo in self.code_health_repos() {
            let since = marker(&self.places.state, &repo);
            let fresh = saved.get(&repo).filter(|read| now - read.at < wait && read.since == since);
            let reasons = match fresh {
                Some(read) => read.reasons.clone(),
                None => {
                    let Some(reasons) = self.code_health_signal(&repo) else { continue };
                    if self.profiling {
                        saved.insert(repo.clone(), Read { at: now, since: since.clone(), reasons: reasons.clone() });
                    }
                    reasons
                }
            };
            let cost = told.reviews.get(&repo).and_then(|review| Some((review.hand_files_median?, review.revert_share?)));
            readings.push(Reading { repo, since, reasons, cost });
        }
        if self.profiling {
            let _ = fs::create_dir_all(self.places.flow());
            if let Err(error) = serde_json::to_string(&saved).map_err(std::io::Error::other).and_then(|text| write_atomic(&path, &text)) {
                eprintln!("detectors: could not record the code-health readings: {error}");
            }
        }
        code_health_review::Facts { readings }
    }
}
