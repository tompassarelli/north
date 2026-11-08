use std::path::Path;

use north_land_probes::RefusalRate;
use serde::{Deserialize, Serialize};

use crate::land::queue;

const REFUSALS: &str = "refusals";
const KEPT: usize = 32;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Refusals {
    decisions: Vec<bool>,
}

impl Refusals {
    pub(super) fn load(queue: &Path) -> Self {
        std::fs::read_to_string(queue.join(REFUSALS)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn save(&self, queue: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        queue::write_atomic(&queue.join(REFUSALS), &text).map_err(|error| error.to_string())
    }

    pub(super) fn record(queue: &Path, refused: bool, lanes: usize) -> Result<(), String> {
        let mut refusals = Self::load(queue);
        for _ in 0..lanes {
            refusals.observe(refused);
        }
        refusals.save(queue)
    }

    fn observe(&mut self, refused: bool) {
        self.decisions.push(refused);
        let over = self.decisions.len().saturating_sub(KEPT);
        self.decisions.drain(..over);
    }

    pub(super) fn rate(&self) -> RefusalRate {
        self.decisions.iter().fold(RefusalRate::default(), |rate, refused| rate.observe(*refused))
    }
}
