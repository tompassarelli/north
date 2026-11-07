pub fn reserved_cpus(profile: &str, cores: f64) -> f64 {
    match profile {
        "attended" if cores > 8.0 => 4.0,
        "attended" if cores > 4.0 => 2.0,
        "attended" => 1.0,
        _ => 0.0,
    }
}

/// The CPUs leases may hold: the agent.slice ceiling the kernel enforces when one is set, which already keeps the desktop's threads outside agent.slice, else the cores less the profile's reserve.
pub fn aggregate_cpus(profile: &str, cores: f64, slice_cpus: Option<f64>) -> f64 {
    let usable = slice_cpus.filter(|ceiling| ceiling.is_finite() && *ceiling > 0.0).map_or_else(|| cores - reserved_cpus(profile, cores), |ceiling| cores.min(ceiling));
    usable.max(1.0)
}
