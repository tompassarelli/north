//! Each host's count of transient north processes older than the leak window across settle ticks, so settle calls a
//! host broken when processes outlive their work and never when it is only busy: a leak raises the fewest such processes
//! the host runs, while work that ends within the window never enters the count.
use super::layout::Layout;
use super::probe::default_number;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Sample {
    at: u64,
    count: u64,
}

pub(super) struct Growth {
    path: PathBuf,
    window: u64,
    slices: u64,
    hosts: BTreeMap<String, Vec<Sample>>,
}

impl Growth {
    pub(super) fn load(layout: &Layout) -> Self {
        let path = layout.state.join("runtime-settle-growth.json");
        let hosts = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        Self { path, window: default_number("north_process_leak_window_sec") as u64, slices: default_number("north_process_leak_slices") as u64, hosts }
    }

    pub(super) fn note(&mut self, host: &str, at: u64, count: u64) {
        let samples = self.hosts.entry(host.to_owned()).or_default();
        samples.push(Sample { at, count });
        samples.retain(|sample| sample.at + self.window > at);
    }

    fn width(&self) -> u64 {
        self.window / self.slices
    }

    fn floors(&self, host: &str, now: u64) -> Option<Vec<u64>> {
        let samples = self.hosts.get(host)?;
        let start = now.checked_sub(self.window)?;
        let width = self.width();
        (0..self.slices).map(|slice| samples.iter().filter(|sample| sample.at > start + slice * width && sample.at <= start + (slice + 1) * width).map(|sample| sample.count).min()).collect()
    }

    pub(super) fn leak(&self, host: &str, now: u64) -> Option<String> {
        let floors = self.floors(host, now)?;
        floors.windows(2).all(|pair| pair[1] > pair[0]).then(|| format!("transient north processes leak: the fewest running over {} min in each {} min of the last {} min rose every time", self.window / 60, self.width() / 60, self.window / 60))
    }

    pub(super) fn save(&self) -> std::io::Result<()> {
        fs::write(&self.path, serde_json::to_string(&self.hosts)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Growth, Layout};

    const MIDNIGHT_2026_11_08: u64 = 1_794_070_800;
    const RETIRED_BOUND: u64 = 100;

    fn growth() -> (tempfile::TempDir, Growth) {
        let state = tempfile::tempdir().unwrap();
        let growth = Growth::load(&Layout { state: state.path().to_owned() });
        (state, growth)
    }

    fn readings() -> Vec<(u64, u64)> {
        include_str!("fixtures/settle-whiterabbit-2026-11-08.txt")
            .lines()
            .map(|line| {
                let (clock, said) = line.split_once(' ').unwrap();
                let at = clock.split(':').fold(0, |seconds, part| seconds * 60 + part.parse::<u64>().unwrap());
                (MIDNIGHT_2026_11_08 + at, said.parse().unwrap_or(RETIRED_BOUND))
            })
            .collect()
    }

    #[test]
    fn whiterabbit_busy_with_offloaded_land_checks_on_2026_11_08_is_not_a_leak() {
        let (_state, mut growth) = growth();
        let readings = readings();
        assert_eq!(readings.iter().filter(|(_, count)| *count > RETIRED_BOUND).count(), 20, "the retired snapshot bound refused 20 of these ticks");
        for (at, count) in readings {
            growth.note("whiterabbit", at, count);
            assert_eq!(growth.leak("whiterabbit", at), None, "reading {count} at {at}");
        }
    }

    #[test]
    fn a_count_that_grows_across_every_sample_of_its_window_is_a_leak() {
        let (_state, mut growth) = growth();
        let end = readings().last().unwrap().0;
        for (tick, at) in (end - growth.window..=end).step_by(30).enumerate() {
            growth.note("whiterabbit", at, 3 + tick as u64);
        }
        assert_eq!(growth.floors("whiterabbit", end), Some(vec![4, 24, 44, 64, 84, 104]));
        assert_eq!(growth.leak("whiterabbit", end).as_deref(), Some("transient north processes leak: the fewest running over 60 min in each 10 min of the last 60 min rose every time"));
    }
}
