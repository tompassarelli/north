use super::plan;

const WEIGHT: f64 = 0.25;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RefusalRate(f64);

impl RefusalRate {
    #[must_use]
    pub fn observe(self, refused: bool) -> Self {
        Self(self.0 + WEIGHT * (f64::from(u8::from(refused)) - self.0))
    }

    #[must_use]
    pub fn stride(self) -> usize {
        if self.0 <= 0.0 {
            return usize::MAX;
        }
        let width = (1.0 / self.0.sqrt()).floor();
        if width >= usize::MAX as f64 { usize::MAX } else { (width as usize).max(1) }
    }
}

pub fn urgent_first<L: Clone + PartialEq>(queue: &[L], urgent: &[L]) -> Vec<L> {
    queue.iter().filter(|lane| urgent.contains(lane)).chain(queue.iter().filter(|lane| !urgent.contains(lane))).cloned().collect()
}

pub fn paced<L: Clone + PartialEq>(queue: &[L], urgent: &[L], slots: usize, rate: RefusalRate, groups: &[Vec<L>]) -> Vec<Vec<L>> {
    let ordered = urgent_first(queue, urgent);
    let cap = rate.stride();
    let head = ordered.first().filter(|lane| slots > 0 && urgent.contains(lane) && !groups.iter().any(|group| group.contains(lane))).map(|lane| vec![lane.clone()]);
    let Some(head) = head else {
        return plan(&ordered, slots, cap, groups);
    };
    let mut probes = vec![head];
    for probe in plan(&ordered, slots - 1, cap, groups) {
        if !probes.contains(&probe) {
            probes.push(probe);
        }
    }
    probes
}

#[cfg(test)]
#[path = "pace_tests.rs"]
mod tests;
