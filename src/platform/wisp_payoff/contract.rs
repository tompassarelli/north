use super::{Counts, Run};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Family {
    pub id: String,
    pub scripts: Vec<String>,
    pub match_rule: ScriptMatch,
    pub verdict: &'static str,
    pub native_sample_rate: f64,
    pub counts: Counts,
    pub window_start_unix: i64,
    pub window_end_unix: i64,
}
#[derive(Debug, Serialize)]
pub struct ScriptMatch {
    pub kind: &'static str,
    pub values: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Sampler {
    pub identity: &'static str,
    pub input: &'static str,
    pub native_when: &'static str,
    pub example_script: &'static str,
    pub example_revision: &'static str,
    pub example_bucket: u32,
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            identity: "sha256-first-u32be-mod10-v1",
            input: "UTF-8(repo-relative POSIX script path + LF + full source revision); no trailing LF; reject absolute paths, dot segments and backslashes",
            native_when: "verdict=native OR bucket=0; bucket is first 4 SHA-256 bytes as unsigned big-endian integer modulo 10",
            example_script: "test/native/pads/448/lich.pad",
            example_revision: "0123456789abcdef0123456789abcdef01234567",
            example_bucket: bucket(
                "test/native/pads/448/lich.pad",
                "0123456789abcdef0123456789abcdef01234567",
            ),
        }
    }
}
fn bucket(script: &str, revision: &str) -> u32 {
    let digest = Sha256::digest(format!("{script}\n{revision}").as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 10
}
pub(super) fn families(runs: &[Run], now: i64) -> Vec<Family> {
    let mut counts: BTreeMap<String, Counts> = BTreeMap::new();
    let mut scripts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for run in runs
        .iter()
        .filter(|r| r.at >= now - 14 * 86400 && r.at <= now)
    {
        counts.entry(run.kind.clone()).or_default().add(run);
        if let Some(script) = &run.script {
            scripts
                .entry(run.kind.clone())
                .or_default()
                .insert(script.clone());
        }
    }
    counts
        .into_iter()
        .map(|(id, counts)| {
            let retired = counts.agrees >= 10 && counts.disagrees == 0;
            let scripts: Vec<_> = scripts
                .remove(&id)
                .unwrap_or_default()
                .into_iter()
                .collect();
            let parents: BTreeSet<_> = scripts
                .iter()
                .filter_map(|s| Path::new(s).parent())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let match_rule = if id.starts_with("accept/") {
                ScriptMatch {
                    kind: "none",
                    values: vec![],
                }
            } else if parents.iter().all(|p| p.ends_with("/pads")) {
                ScriptMatch {
                    kind: "exact-path",
                    values: scripts.clone(),
                }
            } else {
                ScriptMatch {
                    kind: "parent-directory",
                    values: parents.into_iter().collect(),
                }
            };
            Family {
                id,
                scripts,
                match_rule,
                verdict: if retired { "wisp-only" } else { "native" },
                native_sample_rate: if retired { 0.1 } else { 1.0 },
                counts,
                window_start_unix: now - 14 * 86400,
                window_end_unix: now,
            }
        })
        .collect()
}
