//! Box durations: the worker runs on one item, chained by how soon each was staffed after the last ended.
use std::collections::BTreeMap;

use super::{Knobs, quantile};
use crate::foundation::clock::Timestamp;
use crate::threads::runs::brief::CATEGORIES;

/// A finished worker run on an item, as the runs table holds it.
#[derive(Clone, Debug, Default)]
pub(crate) struct BoxRun {
    pub item: String,
    pub category: String,
    pub outcome: String,
    pub staffed: Option<String>,
    pub started: Option<String>,
    pub ended: String,
    pub landed: bool,
    pub landed_at: Option<String>,
}

/// One box: its first staffing to its last landing, the runs it took, and its category when known.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoxChain {
    pub item: String,
    pub category: Option<String>,
    pub start: Timestamp,
    pub end: Timestamp,
    pub runs: usize,
    pub minutes: f64,
}

/// A time column as bin/threads read it: a naive stamp is UTC; None when the text is no stamp.
pub(crate) fn moment(text: &str) -> Option<Timestamp> {
    Timestamp::parse_loose(text)
}

fn minutes_between(from: Timestamp, to: Timestamp) -> f64 {
    (to.unix_nanos() - from.unix_nanos()) as f64 / 60e9
}

/// The runs of each item in staffed order, split where a run is staffed more than `gap` minutes after the latest end so
/// far; a chain is a box only when one of its runs landed. Each run is paired with when it began.
fn box_groups(runs: &[BoxRun], gap: f64) -> Vec<Vec<(Timestamp, &BoxRun)>> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_item: BTreeMap<&str, Vec<(Timestamp, &BoxRun)>> = BTreeMap::new();
    for run in runs.iter().filter(|run| !run.item.is_empty() && run.outcome != "split" && !run.ended.is_empty()) {
        let begin = run.staffed.as_deref().or(run.started.as_deref()).filter(|text| !text.is_empty()).unwrap_or(&run.ended);
        let (Some(begin), Some(_)) = (moment(begin), moment(&run.ended)) else { continue };
        if !by_item.contains_key(run.item.as_str()) {
            order.push(&run.item);
        }
        by_item.entry(&run.item).or_default().push((begin, run));
    }
    let mut groups: Vec<Vec<(Timestamp, &BoxRun)>> = Vec::new();
    for item in order {
        let mut rs = by_item.remove(item).unwrap_or_default();
        rs.sort_by_key(|(begin, _)| *begin);
        let mut end: Option<Timestamp> = None;
        for (begin, run) in rs {
            if end.is_none_or(|end| minutes_between(end, begin) > gap) {
                groups.push(Vec::new());
                end = None;
            }
            groups.last_mut().expect("a chain").push((begin, run));
            let ended = moment(&run.ended).expect("ended parsed above");
            end = Some(end.map_or(ended, |end| end.max(ended)));
        }
    }
    groups.retain(|group| group.iter().any(|(_, run)| run.landed));
    groups
}

/// Box durations by item, ordered by landing: a chain with a landed run lasts from its first staffing to its last landing.
pub(crate) fn box_chains(knobs: &Knobs, runs: &[BoxRun]) -> Vec<BoxChain> {
    let mut chains: Vec<BoxChain> = box_groups(runs, knobs.box_chain_gap_min)
        .into_iter()
        .map(|group| {
            let finish = group.iter().filter(|(_, run)| run.landed).filter_map(|(_, run)| moment(run.landed_at.as_deref().filter(|text| !text.is_empty()).unwrap_or(&run.ended))).max().expect("a landed run");
            let (start, first) = group[0];
            BoxChain {
                item: first.item.clone(),
                category: Some(first.category.clone()).filter(|category| CATEGORIES.contains(&category.as_str())),
                start,
                end: finish,
                runs: group.len(),
                minutes: minutes_between(start, finish).max(0.0),
            }
        })
        .collect();
    chains.sort_by_key(|chain| chain.end);
    chains
}

/// (p75 minutes, n, label): the category's chains when it has `eta_min_samples`, else every category's.
pub(crate) fn box_duration(knobs: &Knobs, chains: &[BoxChain], category: Option<&str>) -> (Option<f64>, usize, String) {
    if let Some(category) = category.filter(|category| CATEGORIES.contains(category)) {
        let minutes: Vec<f64> = chains.iter().filter(|chain| chain.category.as_deref() == Some(category)).map(|chain| chain.minutes).collect();
        if minutes.len() >= knobs.eta_min_samples {
            return (quantile(&minutes, 0.75), minutes.len(), format!("box p75 Category: {category}"));
        }
    }
    let minutes: Vec<f64> = chains.iter().map(|chain| chain.minutes).collect();
    (quantile(&minutes, 0.75), minutes.len(), "box p75 all categories".to_owned())
}
