//! GPU admission's measures: the BAR1 aperture, client caps, slots and VRAM headroom a starting render needs.

use super::{Class, Facts, Profile, default_mib};

const ATTENDED_GPU_CLIENTS: u32 = 4;
pub(crate) const GPU_BUSY_PERCENT: u32 = 85;

pub fn starts_gpu_work(facts: &Facts) -> bool {
    match facts.class {
        Class::Native => facts.new_clients > 0 || facts.new_desktops > 0,
        Class::Gpu => true,
        _ => false,
    }
}

pub(super) fn bar1_need_mib(facts: &Facts) -> f64 {
    let clients = match facts.class {
        Class::Gpu => 1,
        _ => facts.new_clients,
    };
    aperture_need_mib(facts.new_desktops, clients, facts.host_bar1_margin_mib)
}

pub fn aperture_need_mib(desktops: u32, clients: u32, host_margin_mib: Option<u32>) -> f64 {
    let margin = host_margin_mib.map_or_else(|| default_mib("bar1_margin_mib"), f64::from);
    f64::from(desktops) * default_mib("bar1_desktop_mib") + f64::from(clients) * default_mib("bar1_client_mib") + margin
}

pub(crate) fn aperture_short(facts: &Facts) -> bool {
    starts_gpu_work(facts) && facts.bar1_free_mib.is_some_and(|free| free < bar1_need_mib(facts))
}

/// The Warcraft clients the host takes: its static count as a ceiling, lowered to the clients it
/// runs plus those its measured free BAR1 and VRAM still fit.
pub fn warcraft_client_cap(facts: &Facts) -> Option<u32> {
    let ceiling = facts.host_warcraft_clients.or((facts.profile == Profile::Attended).then_some(ATTENDED_GPU_CLIENTS));
    let fits = |free: Option<f64>, reserve: f64, each: f64| free.map(|free| ((free - reserve) / each).floor().max(0.0) as u32);
    let bar1_margin = facts.host_bar1_margin_mib.map_or_else(|| default_mib("bar1_margin_mib"), f64::from);
    let bar1 = fits(facts.bar1_free_mib, bar1_margin, default_mib("bar1_client_mib"));
    let vram = fits(facts.vram_free_mib, default_mib("vram_margin_mib"), default_mib("vram_client_mib"));
    let measured = bar1.into_iter().chain(vram).min().map(|more| facts.gpu_clients.saturating_add(more));
    match (ceiling, measured) {
        (Some(ceiling), Some(measured)) => Some(ceiling.min(measured)),
        (ceiling, measured) => ceiling.or(measured),
    }
}

/// Whether `new_clients` more clients, beside the running ones and the `held` slots admitted leases keep for theirs, pass the cap.
pub(super) fn over_client_cap(facts: &Facts, new_clients: u32, held: u32) -> bool {
    warcraft_client_cap(facts).is_some_and(|cap| facts.gpu_clients.saturating_add(held).saturating_add(new_clients) > cap)
}

/// The gpu leases the host runs at once: its measured `gpu_slots` in `[hosts.NAME]`, else the default sized for the laptop.
pub fn gpu_slots(host_gpu_slots: Option<u32>) -> u32 {
    host_gpu_slots.unwrap_or_else(|| default_mib("gpu_slots") as u32)
}

/// Whether one more render, beside the gpu leases still ramping to their VRAM, would leave less free VRAM than the headroom Warcraft clients keep.
pub(super) fn gpu_memory_short(facts: &Facts) -> bool {
    let ramp = default_mib("gpu_vram_ramp_seconds");
    let ramping = facts.leases.iter().filter(|lease| lease.class == "gpu" && (facts.now - lease.created_at) / 1000.0 < ramp).count();
    let need = (ramping + 1) as f64 * default_mib("vram_gpu_run_mib");
    facts.vram_free_mib.is_some_and(|free| free - need < default_mib("vram_gpu_headroom_mib"))
}
