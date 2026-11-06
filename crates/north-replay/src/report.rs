//! What a replay found: each kind's outcome counts, logged beside replayed, and each invariant.
use std::collections::BTreeMap;

use north_base::foundation::clock::Timestamp;
use north_strategy_types::github::Quota;

use crate::bounds::Bounds;

pub fn stamp(at: f64) -> String {
    Timestamp::from_unix_seconds_f64(at).iso()
}

#[derive(Default)]
pub struct Counts {
    pub kind: &'static str,
    pub cases: usize,
    pub unreplayable: usize,
    pub logged: BTreeMap<String, usize>,
    pub replayed: BTreeMap<String, usize>,
}

impl Counts {
    pub fn new(kind: &'static str) -> Self {
        Self { kind, ..Self::default() }
    }

    pub fn record(&mut self, logged: &str, replayed: Option<&str>) {
        self.cases += 1;
        *self.logged.entry(logged.to_owned()).or_default() += 1;
        match replayed {
            Some(label) => *self.replayed.entry(label.to_owned()).or_default() += 1,
            None => self.unreplayable += 1,
        }
    }

    /// The outcomes whose replayed count left the bound around the logged count.
    pub fn shifts(&self, bounds: &Bounds) -> Vec<String> {
        let labels: std::collections::BTreeSet<&String> = self.logged.keys().chain(self.replayed.keys()).collect();
        labels
            .into_iter()
            .filter_map(|label| {
                let (logged, replayed) = (self.logged.get(label).copied().unwrap_or(0), self.replayed.get(label).copied().unwrap_or(0));
                let (low, high) = (logged.min(replayed), logged.max(replayed));
                (high >= bounds.shift_min_cases && high as f64 > bounds.shift_ratio * low.max(1) as f64)
                    .then(|| format!("{} {label}: logged {logged}, replayed {replayed}, past the x{} bound of replay_outcome_shift_ratio", self.kind, bounds.shift_ratio))
            })
            .collect()
    }

    pub fn merge(&mut self, other: Self) {
        self.cases += other.cases;
        self.unreplayable += other.unreplayable;
        for (label, count) in other.logged {
            *self.logged.entry(label).or_default() += count;
        }
        for (label, count) in other.replayed {
            *self.replayed.entry(label).or_default() += count;
        }
    }

    pub fn line(&self) -> String {
        let side = |counts: &BTreeMap<String, usize>| counts.iter().map(|(label, count)| format!("{label} {count}")).collect::<Vec<_>>().join(", ");
        let skipped = if self.unreplayable == 0 { String::new() } else { format!(" ({} unreplayable)", self.unreplayable) };
        format!("{}: {} cases{skipped}\n  logged   {}\n  replayed {}", self.kind, self.cases, side(&self.logged), side(&self.replayed))
    }
}

pub struct Invariant {
    pub name: &'static str,
    pub rule: String,
    pub checked: usize,
    pub passed: usize,
    pub breaches: Vec<String>,
}

impl Invariant {
    pub fn new(name: &'static str, rule: String) -> Self {
        Self { name, rule, checked: 0, passed: 0, breaches: Vec::new() }
    }

    pub fn check(&mut self, held: bool, breach: impl FnOnce() -> String) {
        self.checked += 1;
        if held {
            self.passed += 1;
        } else {
            self.breaches.push(breach());
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.checked += other.checked;
        self.passed += other.passed;
        self.breaches.extend(other.breaches);
    }

    pub fn line(&self) -> String {
        let verdict = if self.breaches.is_empty() { "ok" } else { "BREACHED" };
        let applicable = if self.checked == 0 { "; no logged case in the window applied, so only the planted-case test guards it" } else { "" };
        let mut text = format!("{}: {} of {} checked pass, {verdict} ({}{applicable})", self.name, self.passed, self.checked, self.rule);
        for breach in self.breaches.iter().take(3) {
            text.push_str(&format!("\n  {breach}"));
        }
        if self.breaches.len() > 3 {
            text.push_str(&format!("\n  and {} more", self.breaches.len() - 3));
        }
        text
    }
}

pub const GITHUB_FLOOR: &str = "github-floor";

/// The invariant that a launch placed or a round landed leaves the floor of GitHub calls unspent.
pub fn github_floor(floor: f64) -> Invariant {
    Invariant::new(GITHUB_FLOOR, format!("a launch is placed and a round lands only while the least GitHub rate limit the broker had seen keeps {floor} calls"))
}

impl Invariant {
    /// Judges one decision against the lowest GitHub reading its case logged; a case without one does not apply.
    pub fn check_github(&mut self, quota: Option<&Quota>, floor: f64, at: f64, what: impl FnOnce() -> String) {
        let Some(quota) = quota else { return };
        self.check(quota.remaining as f64 >= floor, || format!("{}: {} with {} GitHub {} calls left, under the floor of {floor}", stamp(at), what(), quota.remaining, quota.resource));
    }
}
