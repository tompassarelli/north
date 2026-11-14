//! The one dispatch from a detector kind to the facts it reads and the check that files its findings.
use super::{Detector, Finding, Gatherer, Limits, code_health_review, correction, crate_headroom, disk, followed_source, machine_fix, machine_refusal, mechanism_discovery, plan, refusal, signal, skill_review, slow_check, slow_test};

pub fn run(detector: Detector, gatherer: &Gatherer, now: f64, limits: &Limits, pages: &mut Vec<Finding>) -> Option<Vec<Finding>> {
    Some(match detector {
        Detector::StalledPlan => {
            let facts = gatherer.plans(now);
            pages.extend(plan::unstaffed(&facts, now, limits));
            plan::stalled(&facts, now, limits)
        }
        Detector::SilentSignal => signal::silent(&gatherer.signals(), now, limits),
        Detector::RepeatedRefusal => refusal::repeated(&gatherer.refusals(now, limits.refusal_window_s), now, limits),
        Detector::LowDisk => disk::low(&gatherer.disks(), now, limits),
        Detector::RecurringSequence => return None,
        Detector::RepeatedCorrection => correction::repeated(&gatherer.corrections(), crate::threads::incidents::corrections::classes()),
        Detector::SlowTest => slow_test::slow(&gatherer.slow_tests(limits, now), limits),
        Detector::SlowCheck => slow_check::slow(&slow_check::over(&gatherer.check_samples(), &slow_check::budgets(), limits.slow_test_runs)),
        Detector::CrateHeadroom => crate_headroom::near(&gatherer.crates()),
        Detector::MachineFix => machine_fix::fixed(&gatherer.machine_fixes()),
        Detector::MachineRefusal => machine_refusal::refused(&gatherer.machine_refusals()),
        Detector::SkillReview => skill_review::due(&gatherer.skills()),
        Detector::CodeHealthReview => code_health_review::due(&gatherer.code_health_readings(now)),
        Detector::FollowedSource => followed_source::due(&gatherer.followed_sources(now), now),
        Detector::MechanismDiscovery => mechanism_discovery::due(&gatherer.mechanism_discovery(now)),
    })
}
