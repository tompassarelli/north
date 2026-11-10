//! The wait a detector's subject learns from yield: the filer holds a subject's next box until its last box's wait has
//! passed, and the wait doubles when that box closed with nothing changed and returns to the detector's floor when it led
//! to a change.
use super::{Detector, default_number};

pub struct Policy {
    pub floor_s: f64,
    pub doubles: bool,
}

/// The lifecycle-machine detectors learn a wait per machine from `detector_machine_review_floor_hours`. A refused move
/// is the run disagreeing with the model, so it never lengthens the wait.
pub fn policy(detector: Detector) -> Option<Policy> {
    let floor_s = || default_number("detector_machine_review_floor_hours") * 3600.0;
    match detector {
        Detector::MachineFix => Some(Policy { floor_s: floor_s(), doubles: true }),
        Detector::MachineRefusal => Some(Policy { floor_s: floor_s(), doubles: false }),
        _ => None,
    }
}

/// Whether a ticked box line names a commit, which is how a closed box shows it led to a change.
pub fn names_commit(line: &str) -> bool {
    line.split(|byte: char| !byte.is_ascii_alphanumeric()).any(|word| (7..=40).contains(&word.len()) && word.bytes().all(|byte| byte.is_ascii_hexdigit()) && word.bytes().any(|byte| byte.is_ascii_digit()))
}

/// The wait before a subject's next box, given the wait its last box was filed after and whether that box closed
/// having changed something.
pub fn after(policy: &Policy, last_wait_s: f64, changed: bool) -> f64 {
    if changed || !policy.doubles { policy.floor_s } else { last_wait_s * 2.0 }
}

pub fn describe(seconds: f64) -> String {
    format!("{:.1} days", seconds / 86400.0)
}
