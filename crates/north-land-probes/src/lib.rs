//! Which candidates the lander checks in one round and which one it lands (north#842): cumulative prefixes of the
//! queue spaced across the free slots; the suspects of a red round split in halves, quarters and so on, each probed with
//! the clear lanes, so k bad lanes among n are isolated in about k log2(n) probes. A landed
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
    pub refuse: Vec<L>,
    pub groups: Vec<Vec<L>>,
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

fn halves<L: Clone>(suspects: &[L]) -> Vec<Vec<L>> {
    if suspects.len() == 1 {
        return vec![suspects.to_vec()];
    }
    let mut segments: Vec<Vec<L>> = Vec::new();
    let mut next = 0;
    let mut split: &[L] = suspects;
    loop {
        if split.len() > 1 {
            let (first, second) = split.split_at(split.len() / 2);
            segments.extend([first.to_vec(), second.to_vec()]);
        }
        let Some(segment) = segments.get(next) else { break };
        split = segment;
        next += 1;
    }
    segments
}

fn with_clear<L: Clone + PartialEq>(queue: &[L], clear: &[L], segment: &[L], cap: usize) -> Vec<L> {
    let room = cap.saturating_sub(segment.len());
    queue.iter().filter(|lane| segment.contains(lane) || clear.iter().take(room).any(|kept| kept == *lane)).cloned().collect()
}

pub fn plan<L: Clone + PartialEq>(queue: &[L], slots: usize, cap: usize, groups: &[Vec<L>]) -> Vec<Vec<L>> {
    let named = |lane: &L| groups.iter().any(|group| group.contains(lane));
    let clear: Vec<L> = queue.iter().filter(|lane| !named(lane)).cloned().collect();
    let splits: Vec<Vec<Vec<L>>> = groups.iter().map(|group| queue.iter().filter(|lane| group.contains(lane)).cloned().collect::<Vec<L>>()).filter(|group| !group.is_empty()).map(|group| halves(&group)).collect();
    let deepest = splits.iter().map(Vec::len).max().unwrap_or(0);
    let segments: Vec<&Vec<L>> = (0..deepest).flat_map(|at| splits.iter().filter_map(move |split| split.get(at))).collect();
    let reserve = usize::from(!clear.is_empty() && slots > 1);
    let mut probes: Vec<Vec<L>> = Vec::new();
    for segment in &segments {
        if probes.len() + reserve >= slots {
            break;
        }
        let probe = if segment.len() == 1 && reserve == 0 { (*segment).clone() } else { with_clear(queue, &clear, segment, cap) };
        if !probe.is_empty() && !probes.contains(&probe) {
            probes.push(probe);
        }
    }
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
    for segment in segments.iter().filter(|_| !clear.is_empty()) {
        if probes.len() >= slots {
            break;
        }
        let alone: Vec<L> = segment.iter().take(cap).cloned().collect();
        if !probes.contains(&alone) {
            probes.push(alone);
        }
    }
    probes
}

pub fn cancelled<L: PartialEq>(probes: &[Vec<L>], red: usize, running: &[bool]) -> Vec<usize> {
    let red_probe = &probes[red];
    let inner = probes.iter().filter(|probe| probe.len() < red_probe.len() && probe.iter().all(|lane| red_probe.contains(lane))).max_by_key(|probe| probe.len());
    let stride: Vec<&L> = red_probe.iter().filter(|lane| inner.is_none_or(|inner| !inner.contains(lane))).collect();
    probes.iter().enumerate().filter(|(at, probe)| *at != red && running.get(*at).copied().unwrap_or(false) && !probe.iter().all(|lane| red_probe.contains(lane)) && stride.iter().all(|lane| probe.contains(lane))).map(|(at, _)| at).collect()
}

pub fn superseded<L: PartialEq>(probes: &[Vec<L>], green: usize, running: &[bool]) -> Vec<usize> {
    let landing = &probes[green];
    probes.iter().enumerate().filter(|(at, probe)| *at != green && running.get(*at).copied().unwrap_or(false) && probe.iter().all(|lane| landing.contains(lane))).map(|(at, _)| at).collect()
}

pub fn settle<L: Clone + PartialEq>(probes: &[Vec<L>], green: &[bool], groups: &[Vec<L>]) -> Settled<L> {
    let mut land: &[L] = &[];
    for (probe, _) in probes.iter().zip(green).filter(|(_, green)| **green) {
        if probe.len() > land.len() {
            land = probe;
        }
    }
    let red: Vec<&Vec<L>> = probes.iter().zip(green).filter(|(_, green)| !**green).map(|(probe, _)| probe).collect();
    let cleared: Vec<&L> = probes.iter().zip(green).filter(|(_, green)| **green).flat_map(|(probe, _)| probe).collect();
    let named = |lane: &L| groups.iter().any(|group| group.contains(lane));
    let mut refuse: Vec<L> = Vec::new();
    for probe in &red {
        if let [culprit] = probe.iter().filter(|lane| !land.contains(lane)).collect::<Vec<_>>()[..]
            && !refuse.contains(culprit)
        {
            refuse.push(culprit.clone());
        }
    }
    let residues: Vec<Vec<L>> = red
        .iter()
        .map(|probe| {
            let outside: Vec<L> = probe.iter().filter(|lane| !land.contains(lane)).cloned().collect();
            let suspected: Vec<L> = outside.iter().filter(|lane| !cleared.contains(lane)).cloned().collect();
            let pointed: Vec<L> = suspected.iter().filter(|lane| named(lane)).cloned().collect();
            if !pointed.is_empty() {
                pointed
            } else if !suspected.is_empty() {
                suspected
            } else {
                outside
            }
        })
        .collect();
    let mut found: Vec<Vec<L>> = Vec::new();
    for residue in &residues {
        let minimal = !residues.iter().any(|other| other.len() < residue.len() && other.iter().all(|lane| residue.contains(lane)));
        let group: Vec<L> = residue.iter().filter(|lane| !refuse.contains(lane)).cloned().collect();
        if minimal && !group.is_empty() && !refuse.iter().any(|culprit| residue.contains(culprit)) && !found.contains(&group) {
            found.push(group);
        }
    }
    let mut next = found.clone();
    for group in groups {
        let narrowed = found.iter().any(|inner| inner.iter().all(|lane| group.contains(lane))) || refuse.iter().any(|culprit| group.contains(culprit));
        let rest: Vec<L> = group.iter().filter(|lane| !land.contains(lane) && !cleared.contains(lane) && !found.iter().any(|inner| inner.contains(lane))).cloned().collect();
        if !narrowed && !rest.is_empty() && !next.contains(&rest) {
            next.push(rest);
        }
    }
    Settled { land: land.to_vec(), refuse, groups: next }
}

#[cfg(test)]
mod tests;
