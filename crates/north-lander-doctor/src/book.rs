use std::collections::BTreeMap;
use std::path::Path;

use north_graph_loops::{Held, Remembered};
use serde::Deserialize;

use crate::bounds::State;
use crate::facts::LinkFacts;

pub const GOOD_KEPT: usize = 5;

/// What the doctor remembers between rounds: the lander builds that landed a batch green, oldest first, each open episode, each bound's last state and the incidents counted by class.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub good: Vec<String>,
    #[serde(default)]
    pub episodes: BTreeMap<String, Episode>,
    #[serde(default)]
    pub states: BTreeMap<String, String>,
    #[serde(default)]
    pub incidents: BTreeMap<String, u64>,
    #[serde(default)]
    pub checked_at: u64,
}

/// One bound held over from `since` until a round reads it within again; each remedy applied in it and the page sent about it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Episode {
    pub since: u64,
    #[serde(default)]
    pub remedies: Vec<(u64, String)>,
    #[serde(default)]
    pub paged: Option<u64>,
}

impl Book {
    /// The book the doctor last told graph.db, empty before its first graph tick.
    pub fn load(graph: &Path) -> Result<Self, String> {
        match north_graph_loops::remembered(graph)? {
            Some(told) if told.at.is_some() => Ok(Self::from(told)),
            _ => Ok(Self::default()),
        }
    }

    /// Tells the book as one transaction and moves the doctor's tick to the round that made it.
    pub fn save(&self, graph: &Path) -> Result<(), String> {
        north_graph_loops::tell_remembered(graph, &self.remembered(), i64::try_from(self.checked_at).unwrap_or(i64::MAX))
    }

    fn remembered(&self) -> Remembered {
        let episodes = self.episodes.iter().map(|(key, episode)| (key.clone(), Held { since: episode.since, remedies: episode.remedies.clone(), paged: episode.paged })).collect();
        Remembered { good: self.good.clone(), episodes, states: self.states.clone(), incidents: self.incidents.clone(), at: None }
    }

    pub fn last_remedy(&self, key: &str, class: &str) -> Option<u64> {
        self.episodes.get(key)?.remedies.iter().filter(|(_, applied)| applied == class).map(|(at, _)| *at).max()
    }

    pub fn open(&mut self, key: &str, now: u64) -> &mut Episode {
        self.episodes.entry(key.to_owned()).or_insert_with(|| Episode { since: now, ..Episode::default() })
    }

    pub fn count_incident(&mut self, class: &str) {
        *self.incidents.entry(class.to_owned()).or_default() += 1;
    }

    pub fn record_state(&mut self, key: String, state: &State) {
        let word = match state {
            State::Healthy => "ok",
            State::Known { .. } => "over",
            State::Unknown(_) => "unknown",
        };
        self.states.insert(key, word.to_owned());
    }

    /// Marks the build the lander link names good once an after-landing run that started after the link last moved passed.
    pub fn learn(&mut self, link: &LinkFacts) {
        if !link.green_since_link || self.good.last() == Some(&link.lander) {
            return;
        }
        self.good.retain(|good| *good != link.lander);
        self.good.push(link.lander.clone());
        let surplus = self.good.len().saturating_sub(GOOD_KEPT);
        self.good.drain(..surplus);
    }
}

impl From<Remembered> for Book {
    fn from(told: Remembered) -> Self {
        let episodes = told.episodes.into_iter().map(|(key, held)| (key, Episode { since: held.since, remedies: held.remedies, paged: held.paged })).collect();
        Book { good: told.good, episodes, states: told.states, incidents: told.incidents, checked_at: told.at.and_then(|at| u64::try_from(at).ok()).unwrap_or_default() }
    }
}
