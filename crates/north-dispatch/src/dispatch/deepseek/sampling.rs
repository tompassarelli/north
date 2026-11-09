//! Which DeepSeek commits a Sonnet reviewer reads (north#1020): every commit of a category until it has
//! `FULL_REVIEWS` clean reviews since its last defect, then one in `SAMPLE_EVERY` by its sha; a landing box always.
//! A reviewer's defect or the lander's refusal of an unreviewed commit puts its category back on full review.
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub const FULL_REVIEWS: usize = 10;
pub const SAMPLE_EVERY: u64 = 5;
const FILE: &str = "deepseek-verdicts.jsonl";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A reviewer passed it.
    Clean,
    /// A reviewer found a real defect, named by its class.
    Defect,
    /// Sampling sent it to safe-push without a review, and it queued.
    Unsampled,
    /// Sampling sent it to safe-push without a review, and safe-push refused it.
    Refused,
    /// The session reported Done with no commit ahead of main.
    Empty,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Verdict {
    pub at: f64,
    pub session: String,
    pub category: String,
    pub sha: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

impl Verdict {
    fn resets(&self) -> bool {
        matches!(self.kind, Kind::Defect | Kind::Refused)
    }
}

pub fn read(directory: &Path) -> Vec<Verdict> {
    std::fs::read_to_string(directory.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

pub fn append(directory: &Path, verdict: &Verdict) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(FILE)).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(verdict).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

/// The clean reviews `category` has had since its last defect or refusal, in the order `verdicts` holds them.
pub fn clean_run(verdicts: &[Verdict], category: &str) -> usize {
    verdicts.iter().rev().filter(|verdict| verdict.category == category).take_while(|verdict| !verdict.resets()).filter(|verdict| verdict.kind == Kind::Clean).count()
}

/// Whether the commit `sha` of a box in `category` goes to a reviewer before it lands.
pub fn reviews(verdicts: &[Verdict], category: &str, landing: bool, sha: &str) -> bool {
    landing || clean_run(verdicts, category) < FULL_REVIEWS || sampled(sha)
}

fn sampled(sha: &str) -> bool {
    sha.get(..8).and_then(|head| u64::from_str_radix(head, 16).ok()).is_none_or(|number| number % SAMPLE_EVERY == 0)
}
