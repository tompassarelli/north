use super::admission::{Class, Facts, GPU_BUSY_PERCENT, Profile, Reason, aperture_short, starts_gpu_work};

pub(super) fn deferred(facts: &Facts) -> Option<Reason> {
    if starts_gpu_work(facts) && facts.bar1_timed_out {
        Some(Reason::DeferGpuApertureTimeout)
    } else if aperture_short(facts) {
        Some(Reason::DeferGpuAperture)
    } else if tom_present_gpu_busy(facts) {
        Some(Reason::DeferGpuBusy)
    } else {
        None
    }
}

fn wants_gpu(facts: &Facts) -> bool {
    let named = facts.need.as_deref().is_some_and(|needs| needs.split(',').any(|need| matches!(need.split(':').next(), Some("native" | "private" | "gpu"))));
    starts_gpu_work(facts) || named
}

fn tom_present_gpu_busy(facts: &Facts) -> bool {
    !matches!(facts.class, Class::Agent | Class::Tom) && facts.profile == Profile::Attended && facts.gpu_busy_percent >= GPU_BUSY_PERCENT && facts.gpu_runs == 0 && wants_gpu(facts)
}
