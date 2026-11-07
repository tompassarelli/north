//! The blind-labelled sample the rework judge is scored against: landings drawn at random from its two verdicts, each
//! labelled reworked or not by two readers who saw the landing and the commits within 48 h that touch its files but
//! never the judge's verdict, a third reader settling their disagreements. The judge runs again on each sampled
//! landing's recorded history, so a rule change moves the precision and the recall the sample measures.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use serde::Deserialize;

use super::rework::{Commit, Landing, Verdict, judge_history};

const FIXTURE: &str = include_str!("rework_labels.json");

#[derive(Deserialize)]
struct Fixture {
    population: Population,
    landings: Vec<Sampled>,
}

/// How many closed landings each judge verdict held when the sample was drawn, which weights each stratum.
#[derive(Deserialize)]
struct Population {
    reworked: usize,
    clean: usize,
}

#[derive(Deserialize)]
struct Sampled {
    drawn_reworked: bool,
    item: Option<String>,
    culprit: bool,
    #[serde(flatten)]
    landed: Commit,
    later: Vec<Later>,
    labels: Labels,
}

#[derive(Deserialize)]
struct Later {
    #[serde(flatten)]
    commit: Commit,
    touches: bool,
}

#[derive(Deserialize)]
struct Labels {
    first: bool,
    second: bool,
    settled: Option<bool>,
}

impl Labels {
    fn reworked(&self) -> bool {
        if self.first == self.second { self.first } else { self.settled.expect("a disagreement carries the third reader's label") }
    }
}

impl Sampled {
    fn judged_reworked(&self) -> bool {
        let landing = Landing { agent: self.landed.sha.clone(), item: self.item.clone(), sha: self.landed.sha.clone() };
        let commits: Vec<Commit> = std::iter::once(self.landed.clone()).chain(self.later.iter().map(|later| later.commit.clone())).collect();
        let touching: BTreeSet<&str> = self.later.iter().filter(|later| later.touches).map(|later| later.commit.sha.as_str()).collect();
        let verdict: Option<Verdict> = judge_history(&commits, &[&landing], &|_, later| touching.contains(later.sha.as_str())).pop().map(|(_, verdict)| verdict);
        let culprits: BTreeSet<String> = self.culprit.then(|| self.landed.sha.clone()).into_iter().collect();
        verdict.is_some_and(|verdict| verdict.judged("north", &landing, &culprits).cause.is_some())
    }
}

/// The judge's precision and recall over the whole population, estimated from the sample with each landing weighted by
/// its stratum's size over its draw size, beside how often the two readers agreed before the third settled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub n: usize,
    pub precision: f64,
    pub recall: f64,
    pub agreement: f64,
}

impl Score {
    pub fn line(&self) -> String {
        format!("judge scored against {} blind-labelled north landings: precision {:.0}%, recall {:.0}%, the two readers agreed on {:.0}%", self.n, self.precision * 100.0, self.recall * 100.0, self.agreement * 100.0)
    }
}

static SCORE: LazyLock<Score> = LazyLock::new(|| score_fixture(FIXTURE));

/// The score of the judge as it is now against the committed sample.
pub fn score() -> Score {
    *SCORE
}

fn score_fixture(text: &str) -> Score {
    let fixture: Fixture = serde_json::from_str(text).expect("the committed rework labels parse");
    let drawn = fixture.landings.iter().filter(|sampled| sampled.drawn_reworked).count();
    let weights = [(fixture.population.reworked as f64 / drawn as f64), (fixture.population.clean as f64 / (fixture.landings.len() - drawn) as f64)];
    let (mut hit, mut called, mut real) = (0.0, 0.0, 0.0);
    for sampled in &fixture.landings {
        let weight = weights[usize::from(!sampled.drawn_reworked)];
        let (judged, label) = (sampled.judged_reworked(), sampled.labels.reworked());
        hit += weight * f64::from(u8::from(judged && label));
        called += weight * f64::from(u8::from(judged));
        real += weight * f64::from(u8::from(label));
    }
    let agreed = fixture.landings.iter().filter(|sampled| sampled.labels.first == sampled.labels.second).count();
    Score { n: fixture.landings.len(), precision: hit / called, recall: hit / real, agreement: agreed as f64 / fixture.landings.len() as f64 }
}
