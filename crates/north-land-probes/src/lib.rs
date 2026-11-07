//! Which candidates the lander checks in one round and which one it lands (north#842): cumulative prefixes of the
//! queue spaced across the free slots, then candidates that bypass each suspect of the first red stride. A landed
//! candidate is always one probe whose own merged tree came green. Lanes whose footprints meet share one sub-queue, so
//! lanes of different sub-queues land in the same round without a check of their union. The crate is pure and depends on nothing.
#![forbid(unsafe_code)]

mod pace;
mod subqueue;

pub use pace::{RefusalRate, paced, urgent_first};
pub use subqueue::{Footprint, SubRound, partition, round, share};

#[derive(Debug, PartialEq, Eq)]
pub struct Settled<L> {
    pub land: Vec<L>,
    pub refuse: Option<L>,
    pub suspects: Vec<L>,
}

pub fn prefixes<L: Clone>(lanes: &[L], slots: usize, cap: usize) -> Vec<Vec<L>> {
    let reach = lanes.len().min(cap);
    let count = slots.min(reach);
    if count == 0 {
        return Vec::new();
    }
    let (width, wider) = (reach / count, reach % count);
    let mut end = 0;
    (0..count)
        .map(|stride| {
            end += width + usize::from(stride >= count - wider);
            lanes[..end].to_vec()
        })
        .collect()
}

pub fn plan<L: Clone + PartialEq>(queue: &[L], slots: usize, cap: usize, suspects: &[L]) -> Vec<Vec<L>> {
    let mut probes: Vec<Vec<L>> = suspects
        .iter()
        .take(slots)
        .map(|suspect| queue.iter().filter(|lane| *lane != suspect).take(cap).cloned().collect::<Vec<_>>())
        .filter(|probe| !probe.is_empty())
        .collect();
    let clear: Vec<L> = queue.iter().filter(|lane| !suspects.contains(lane)).cloned().collect();
    for probe in prefixes(&clear, slots - probes.len(), cap) {
        if !probes.contains(&probe) {
            probes.push(probe);
        }
    }
    for skipped in &clear {
        if probes.len() >= slots {
            break;
        }
        let bypass: Vec<L> = clear.iter().filter(|lane| *lane != skipped).take(cap).cloned().collect();
        if !bypass.is_empty() && !probes.contains(&bypass) {
            probes.push(bypass);
        }
    }
    probes
}

pub fn cancelled<L: PartialEq>(probes: &[Vec<L>], red: usize, running: &[bool]) -> Vec<usize> {
    let red_probe = &probes[red];
    let inner = probes.iter().filter(|probe| probe.len() < red_probe.len() && probe.iter().all(|lane| red_probe.contains(lane))).max_by_key(|probe| probe.len());
    let stride: Vec<&L> = red_probe.iter().filter(|lane| inner.is_none_or(|inner| !inner.contains(lane))).collect();
    probes.iter().enumerate().filter(|(at, probe)| *at != red && running.get(*at).copied().unwrap_or(false) && stride.iter().all(|lane| probe.contains(lane))).map(|(at, _)| at).collect()
}

pub fn superseded<L: PartialEq>(probes: &[Vec<L>], green: usize, running: &[bool]) -> Vec<usize> {
    let landing = &probes[green];
    probes.iter().enumerate().filter(|(at, probe)| *at != green && running.get(*at).copied().unwrap_or(false) && probe.iter().all(|lane| landing.contains(lane))).map(|(at, _)| at).collect()
}

pub fn settle<L: Clone + PartialEq>(probes: &[Vec<L>], green: &[bool]) -> Settled<L> {
    let mut land: &[L] = &[];
    for (probe, _) in probes.iter().zip(green).filter(|(_, green)| **green) {
        if probe.len() > land.len() {
            land = probe;
        }
    }
    let narrowest = probes
        .iter()
        .zip(green)
        .filter(|(probe, green)| !**green && land.iter().all(|lane| probe.contains(lane)))
        .map(|(probe, _)| probe)
        .min_by_key(|probe| probe.len());
    let suspects: Vec<L> = narrowest.map_or_else(Vec::new, |probe| probe.iter().filter(|lane| !land.contains(lane)).cloned().collect());
    match suspects.as_slice() {
        [culprit] => Settled { land: land.to_vec(), refuse: Some(culprit.clone()), suspects: Vec::new() },
        _ => Settled { land: land.to_vec(), refuse: None, suspects },
    }
}

#[cfg(test)]
mod tests;
