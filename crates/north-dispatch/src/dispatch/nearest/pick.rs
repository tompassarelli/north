//! The k finished boxes most like a new one and the cheapest rung whose worker-caused outcomes over them
//! clear the success bar.
use super::cause::Cause;
use crate::dispatch::tier::Rung;
use super::shape::Shape;
use std::collections::BTreeMap;

/// Neighbours read per box.
pub const K: usize = 20;
/// A neighbour must be at least this similar to count.
pub const FLOOR: f64 = 0.15;
/// A rung is judged on a box only once this many of its neighbours ran on it with a worker-caused outcome.
pub const MIN_JUDGED: usize = 3;
/// The share of worker-judged neighbours a rung must keep to be picked.
pub const BAR: f64 = 0.8;
/// Pseudo-runs at the rung's pooled fitness that a box's neighbour fitness shrinks toward.
const PRIOR: f64 = 3.0;
const WEIGHTS: Weights = Weights { words: 0.55, areas: 0.25, files: 0.1, repo: 0.1 };

struct Weights {
    words: f64,
    areas: f64,
    files: f64,
    repo: f64,
}

/// A finished run as the neighbour search reads it.
#[derive(Clone, Debug)]
pub struct Finished {
    pub shape: Shape,
    pub rung: Rung,
    pub cause: Cause,
    pub usd: Option<f64>,
}

/// Word vectors weighted by inverse document frequency over the corpus they were built from, unit length.
pub struct Index {
    vectors: Vec<Vec<(u32, f64)>>,
}

impl Index {
    pub fn build(shapes: &[&Shape]) -> Self {
        let mut ids: BTreeMap<&str, u32> = BTreeMap::new();
        let mut frequency: Vec<f64> = Vec::new();
        for shape in shapes {
            for word in shape.words.keys() {
                let next = ids.len() as u32;
                let id = *ids.entry(word.as_str()).or_insert(next);
                if id as usize == frequency.len() {
                    frequency.push(0.0);
                }
                frequency[id as usize] += 1.0;
            }
        }
        let total = shapes.len().max(1) as f64;
        let vectors = shapes
            .iter()
            .map(|shape| {
                let mut vector: Vec<(u32, f64)> = shape.words.iter().map(|(word, count)| (ids[word.as_str()], (1.0 + (*count as f64).ln()) * (total / frequency[ids[word.as_str()] as usize]).ln())).collect();
                vector.sort_by_key(|(id, _)| *id);
                let norm = vector.iter().map(|(_, weight)| weight * weight).sum::<f64>().sqrt();
                vector.iter_mut().for_each(|(_, weight)| *weight /= norm.max(f64::MIN_POSITIVE));
                vector
            })
            .collect();
        Self { vectors }
    }

    fn cosine(&self, a: usize, b: usize) -> f64 {
        let (left, right) = (&self.vectors[a], &self.vectors[b]);
        let (mut i, mut j, mut sum) = (0, 0, 0.0);
        while i < left.len() && j < right.len() {
            match left[i].0.cmp(&right[j].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    sum += left[i].1 * right[j].1;
                    i += 1;
                    j += 1;
                }
            }
        }
        sum
    }

    /// How alike box `query` is to finished box `other`, both indexed here, in [0, 1].
    pub fn similarity(&self, query: (usize, &Shape), other: (usize, &Shape)) -> f64 {
        let (q, n) = (query.1, other.1);
        let covered = |mine: &std::collections::BTreeSet<String>, theirs: &std::collections::BTreeSet<String>| if mine.is_empty() { 0.0 } else { mine.intersection(theirs).count() as f64 / mine.len() as f64 };
        let repo = f64::from(u8::from(q.repo.is_some() && q.repo == n.repo));
        WEIGHTS.words * self.cosine(query.0, other.0) + WEIGHTS.areas * covered(&q.areas, &n.areas) + WEIGHTS.files * covered(&q.files, &n.files) + WEIGHTS.repo * repo
    }
}

/// What a box's neighbours say of one rung: similarity-weighted kept and worker-judged runs, their count,
/// and their median API-equivalent dollars per run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    pub kept: f64,
    pub judged: f64,
    pub judged_runs: usize,
    pub runs: usize,
    pub harness: f64,
    pub usd: Vec<f64>,
}

impl Fit {
    /// Kept share of worker-judged runs, shrunk toward `pooled` with `PRIOR` pseudo-runs.
    pub fn fitness(&self, pooled: f64) -> f64 {
        (self.kept + PRIOR * pooled) / (self.judged + PRIOR)
    }

    pub fn median_usd(&self) -> Option<f64> {
        let mut sorted = self.usd.clone();
        sorted.sort_by(f64::total_cmp);
        sorted.get(sorted.len() / 2).copied()
    }
}

/// Each rung's fit over `neighbours`, each weighted by its similarity.
pub fn fits(neighbours: &[(f64, &Finished)]) -> BTreeMap<Rung, Fit> {
    let mut fits: BTreeMap<Rung, Fit> = BTreeMap::new();
    for (weight, run) in neighbours {
        let fit = fits.entry(run.rung).or_default();
        fit.runs += 1;
        fit.usd.extend(run.usd);
        match run.cause {
            Cause::Kept => {
                fit.kept += weight;
                fit.judged += weight;
                fit.judged_runs += 1;
            }
            Cause::Worker(_) => {
                fit.judged += weight;
                fit.judged_runs += 1;
            }
            Cause::Harness(_) => fit.harness += weight,
        }
    }
    fits
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub rung: Rung,
    pub capped: bool,
    pub why: String,
}

/// The cheapest start rung whose neighbour fitness clears `BAR`, else the one with the least expected dollars
/// per kept box; capped at Sonnet when the box is localized. None when no rung holds `MIN_JUDGED` judged
/// neighbours, which sends the box back to the facet cell.
pub fn pick(query: &Shape, fits: &BTreeMap<Rung, Fit>, pooled: &BTreeMap<Rung, f64>) -> Option<Pick> {
    let fitness = |rung: &Rung, fit: &Fit| fit.fitness(pooled.get(rung).copied().unwrap_or(0.5));
    let judged: Vec<(Rung, f64, Option<f64>)> = Rung::STARTS.iter().filter_map(|rung| fits.get(rung).filter(|fit| fit.judged_runs >= MIN_JUDGED).map(|fit| (*rung, fitness(rung, fit), fit.median_usd()))).collect();
    if judged.is_empty() {
        return None;
    }
    let passing = judged.iter().find(|(_, fit, _)| *fit >= BAR);
    let (rung, why) = match passing {
        Some((rung, fit, _)) => (*rung, format!("{} keeps {:.0}% of its nearest boxes", rung.label(), fit * 100.0)),
        None => {
            let per_kept = |(_, fit, usd): &&(Rung, f64, Option<f64>)| usd.map_or(f64::INFINITY, |usd| usd / fit.max(0.01));
            let best = judged.iter().min_by(|a, b| per_kept(a).total_cmp(&per_kept(b))).expect("judged is not empty");
            (best.0, format!("no rung keeps {:.0}%; {} costs least per kept box among its nearest", BAR * 100.0, best.0.name()))
        }
    };
    let capped = query.localized() && rung > Rung::SONNET;
    Some(if capped { Pick { rung: Rung::SONNET, capped, why: format!("{why}; the brief names its files and symbols, so it starts on sonnet") } } else { Pick { rung, capped, why } })
}
