use std::collections::BTreeSet;

use super::plan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Footprint<K> {
    Everything,
    Crates(BTreeSet<K>),
}

impl<K: Ord> Footprint<K> {
    pub fn meets(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Crates(mine), Self::Crates(theirs)) => !mine.is_disjoint(theirs),
            _ => true,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SubRound<L> {
    pub lanes: Vec<L>,
    pub probes: Vec<Vec<L>>,
}

pub fn partition<L: Clone, K: Ord>(queue: &[(L, Footprint<K>)]) -> Vec<Vec<L>> {
    let mut group: Vec<usize> = (0..queue.len()).collect();
    for later in 0..queue.len() {
        for earlier in 0..later {
            if queue[earlier].1.meets(&queue[later].1) {
                let (from, into) = (group[later].max(group[earlier]), group[later].min(group[earlier]));
                group.iter_mut().filter(|id| **id == from).for_each(|id| *id = into);
            }
        }
    }
    let mut order: Vec<usize> = Vec::new();
    for &id in &group {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    order.into_iter().map(|id| queue.iter().zip(&group).filter(|(_, lane_group)| **lane_group == id).map(|((lane, _), _)| lane.clone()).collect()).collect()
}

pub fn share(lengths: &[usize], slots: usize) -> Vec<usize> {
    let useful: Vec<usize> = lengths.iter().map(|length| (2 * length).saturating_sub(1)).collect();
    let mut shares = vec![0; lengths.len()];
    let mut left = slots;
    while left > 0 {
        let before = left;
        for (taken, room) in shares.iter_mut().zip(&useful) {
            if left > 0 && *taken < *room {
                *taken += 1;
                left -= 1;
            }
        }
        if left == before {
            break;
        }
    }
    shares
}

pub fn round<L: Clone + PartialEq, K: Ord>(queue: &[(L, Footprint<K>)], slots: usize, cap: usize, groups: &[Vec<L>]) -> Vec<SubRound<L>> {
    let queues = partition(queue);
    let shares = share(&queues.iter().map(Vec::len).collect::<Vec<_>>(), slots);
    queues
        .into_iter()
        .zip(shares)
        .filter(|(_, slots)| *slots > 0)
        .map(|(lanes, slots)| {
            let mine: Vec<Vec<L>> = groups.iter().filter(|group| group.iter().all(|lane| lanes.contains(lane))).cloned().collect();
            let probes = plan(&lanes, slots, cap, &mine);
            SubRound { lanes, probes }
        })
        .collect()
}

#[cfg(test)]
#[path = "subqueue_tests.rs"]
mod tests;
