//! The self-detectors: pure functions from recorded state to findings, run by `north flow`. A stalled
//! plan box, a silent signal and a repeated lander refusal class each become one box on the owning
//! issue, filed once per key and left to dispatch to staff.
use std::sync::LazyLock;
use std::time::Instant;

use toml_edit::DocumentMut;

use crate::ledger::Ledger;

use super::flow::Places;

pub use filing::Filer;
pub use gather::Gatherer;

mod disk;
mod filing;
mod gather;
mod plan;
mod refusal;
mod signal;
#[cfg(test)]
mod tests;

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

fn default_number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub fn issue_stall_repos() -> Vec<String> {
    DEFAULTS["detector_issue_stall_repos"]["value"].as_array().into_iter().flatten().filter_map(|repo| repo.as_str().map(str::to_owned)).collect()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Detector {
    StalledPlan,
    SilentSignal,
    RepeatedRefusal,
    LowDisk,
    RecurringSequence,
}

impl Detector {
    pub const ALL: [Self; 5] = [Self::StalledPlan, Self::SilentSignal, Self::RepeatedRefusal, Self::LowDisk, Self::RecurringSequence];

    pub fn name(self) -> &'static str {
        match self {
            Self::StalledPlan => "stalled-plan",
            Self::SilentSignal => "silent-signal",
            Self::RepeatedRefusal => "repeated-refusal",
            Self::LowDisk => "low-disk",
            Self::RecurringSequence => "recurring-sequence",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub detector: Detector,
    pub key: String,
    pub owner: Option<String>,
    pub title: String,
    pub detail: String,
    pub done_when: String,
    pub instances: Vec<String>,
}

impl Finding {
    pub fn box_line(&self) -> String {
        format!("- [ ] **{}.** {} Done when: {} (detector {})", self.title, self.detail, self.done_when, self.key)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub stall_s: f64,
    pub silence_cadences: f64,
    pub refusal_window_s: f64,
    pub refusal_min: usize,
    pub disk_floor_gib: f64,
}

impl Limits {
    pub fn registry() -> Self {
        Self {
            stall_s: default_number("detector_plan_stall_hours") * 3600.0,
            silence_cadences: default_number("detector_silence_cadences"),
            refusal_window_s: default_number("detector_refusal_window_hours") * 3600.0,
            refusal_min: default_number("detector_refusal_min") as usize,
            disk_floor_gib: default_number("build_disk_free_gib"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spend {
    pub detector: Detector,
    pub millis: u64,
    pub findings: usize,
}

pub fn detect_timed(gatherer: &Gatherer, now: f64, limits: &Limits) -> (Vec<Finding>, Vec<Spend>) {
    let mut findings = Vec::new();
    let mut spends = Vec::new();
    for detector in Detector::ALL {
        let started = Instant::now();
        let found = match detector {
            Detector::StalledPlan => plan::stalled(&gatherer.plans(now), now, limits),
            Detector::SilentSignal => signal::silent(&gatherer.signals(), now, limits),
            Detector::RepeatedRefusal => refusal::repeated(&gatherer.refusals(now, limits.refusal_window_s), now, limits),
            Detector::LowDisk => disk::low(&gatherer.disks(), now, limits),
            Detector::RecurringSequence => continue,
        };
        spends.push(Spend { detector, millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX), findings: found.len() });
        findings.extend(found);
    }
    (findings, spends)
}

pub fn record(places: &Places, spends: &[Spend], filed: &[Finding]) -> Result<(), String> {
    let ledger = Ledger::open(&places.state.join("north/ledger.db")).map_err(|error| error.user_message())?;
    for spend in spends {
        let filed = filed.iter().filter(|finding| finding.detector == spend.detector).count();
        ledger.record_detector(spend.detector.name(), spend.millis, spend.findings, filed).map_err(|error| error.user_message())?;
    }
    Ok(())
}

pub fn tick(places: &Places, host: &dyn super::flow::Host, now: f64, file: bool) -> Vec<String> {
    let gatherer = Gatherer::new(places, host);
    let (findings, spends) = detect_timed(&gatherer, now, &Limits::registry());
    let mut lines: Vec<String> = Vec::new();
    let mut filed = Vec::new();
    if file {
        match Filer::new(places, host).file(&findings, now) {
            Ok(done) => {
                for outcome in &done {
                    lines.push(outcome.line());
                }
                filed = done.into_iter().filter(|outcome| outcome.filed).map(|outcome| outcome.finding).collect();
            }
            Err(error) => lines.push(format!("detectors: filing failed: {error}")),
        }
    } else {
        lines.extend(findings.iter().map(|finding| format!("detectors: {} {}: {}", finding.detector.name(), finding.key, finding.title)));
    }
    if file && let Err(error) = record(places, &spends, &filed) {
        lines.push(format!("detectors: ledger not written: {error}"));
    }
    lines.extend(spends.iter().map(|spend| format!("detectors: {} {} ms, {} findings", spend.detector.name(), spend.millis, spend.findings)));
    lines
}
