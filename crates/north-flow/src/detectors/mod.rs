//! The self-detectors: pure functions from recorded state to findings, run by `north ledger flow`. A stalled
//! plan box, a silent signal, a repeated lander refusal class, a class of Tom's corrections seen twice and a test over its
//! time line in its newest land check runs, a crate within 5% of its line cap or over it, a lifecycle machine whose code a landed incident fix changed, a move the lifecycle store refused, a repository whose code-health review signal fires and a skill changed, corrected or drifted since its last review, each become one box on the owning issue, filed once per key and left to dispatch to staff. A later episode of a cause whose box is still open comments its reading on that issue instead of filing a second box.
use std::sync::LazyLock;
use std::time::Instant;

use toml_edit::DocumentMut;

use crate::ledger::Ledger;

use super::flow::Places;

pub use filing::Filer;
pub use gather::Gatherer;
pub use kind::Detector;
pub use slow_test::{Newest, newest as newest_slow_test};

mod code_health_review;
mod correction;
mod crate_headroom;
mod disk;
mod filing;
mod gather;
mod kind;
mod machine_fix;
mod machine_refusal;
mod plan;
mod refusal;
mod signal;
mod skill_review;
mod slow_check;
mod slow_test;
mod waits;

#[cfg(test)]
mod reflex_tests;

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

fn default_number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

fn default_text(name: &str) -> String {
    DEFAULTS[name]["value"].as_str().unwrap_or_else(|| panic!("harness/defaults.toml has no text for {name}")).to_owned()
}

fn default_list(name: &str) -> Vec<String> {
    DEFAULTS[name]["value"].as_array().into_iter().flatten().filter_map(|item| item.as_str().map(str::to_owned)).collect()
}

pub fn issue_stall_repos() -> Vec<String> {
    default_list("detector_issue_stall_repos")
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
    /// What the finding is about, the second part of its key: the machine of a machine finding.
    pub fn subject(&self) -> &str {
        self.key.split(':').nth(1).unwrap_or_default()
    }

    pub fn box_line(&self) -> String {
        let done_when = if self.done_when.starts_with('`') {
            self.done_when.clone()
        } else {
            format!("`north ledger flow detect --dry-run | rg -qF ' {}: '; test $? -eq 1` exits 0, once {}", self.key, self.done_when)
        };
        format!("- [ ] **{}.** {} Done when: {done_when} (detector {})", self.title, self.detail, self.key)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub stall_s: f64,
    pub silence_cadences: f64,
    pub refusal_window_s: f64,
    pub refusal_min: usize,
    pub disk_floor_gib: f64,
    pub slow_test_seconds: f64,
    pub slow_test_runs: usize,
}

impl Limits {
    pub fn registry() -> Self {
        Self {
            stall_s: default_number("detector_plan_stall_hours") * 3600.0,
            silence_cadences: default_number("detector_silence_cadences"),
            refusal_window_s: default_number("detector_refusal_window_hours") * 3600.0,
            refusal_min: default_number("detector_refusal_min") as usize,
            disk_floor_gib: default_number("build_disk_free_gib"),
            slow_test_seconds: default_number("detector_slow_test_seconds"),
            slow_test_runs: default_number("detector_slow_test_runs") as usize,
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
            Detector::RepeatedCorrection => correction::repeated(&gatherer.corrections(), crate::threads::incidents::corrections::classes(), correction::graph_guard_landed),
            Detector::SlowTest => slow_test::slow(&gatherer.slow_tests(limits, now), limits),
            Detector::SlowCheck => slow_check::slow(&slow_check::over(&gatherer.check_samples(), &slow_check::budgets(), limits.slow_test_runs), &default_text("detector_slow_check_owner")),
            Detector::CrateHeadroom => crate_headroom::near(&gatherer.crates()),
            Detector::MachineFix => machine_fix::fixed(&gatherer.machine_fixes()),
            Detector::MachineRefusal => machine_refusal::refused(&gatherer.machine_refusals()),
            Detector::SkillReview => skill_review::due(&gatherer.skills()),
            Detector::CodeHealthReview => code_health_review::due(&gatherer.code_health_readings(now)),
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
    let gatherer = Gatherer::new(places, host).profiling(file);
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
