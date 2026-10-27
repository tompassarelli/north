//! How long a box would wait to start on a host, from the host's live measure and the class of run
//! its Verify facet implies; a host whose memory can never admit that class takes none of it.
use super::measure::Measure;
use crate::capacity::admission::{self, Class};
use crate::ops::pacing::placement::gate;

/// Heavy for a box verified by a local or native test run, which machine-capacity admits as heavy;
/// else agent, since only its CLI loads the host.
pub(crate) fn class(brief: &str) -> Class {
    let verify = gate::words(brief, "Verify");
    if verify.iter().any(|word| matches!(word.as_str(), "local-test" | "native")) { Class::Heavy } else { Class::Agent }
}

/// The expected wait in run lengths: the share of the run queue past the host's cores, plus one run
/// when memory admission would defer the class now; an error when the host's memory cannot admit
/// the class even idle.
pub(crate) fn wait(class: Class, measure: &Measure) -> Result<f64, String> {
    let memory = admission::dispatch_request_mib(class);
    if !admission::memory_admits(measure.memory_total_mib, memory) {
        return Err(format!("cannot admit a {} run of {memory:.0} MiB in {:.0} MiB", format!("{class:?}").to_lowercase(), measure.memory_total_mib));
    }
    let queue = (measure.load / measure.cores.max(1.0) - 1.0).max(0.0);
    let short = admission::memory_short(measure.memory_total_mib, measure.memory_available_mib, memory);
    Ok(queue + if short { 1.0 } else { 0.0 })
}
