use super::admission::{Class, Facts, Profile, default_mib};

pub(super) fn overloaded(facts: &Facts) -> bool {
    let cores = f64::from(facts.cores);
    let claim = match facts.class {
        Class::Heavy => facts.requested_cpus.min(cores / 2.0),
        Class::Exclusive => 0.0,
        _ => return false,
    };
    if facts.cpu_pressure_measured {
        return f64::from(facts.cpu_some_avg10_basis_points) >= pressure_floor_percent(facts.profile) * 100.0;
    }
    facts.run_queue_load.is_some_and(|load| load + claim > cores)
}

fn pressure_floor_percent(profile: Profile) -> f64 {
    default_mib(match profile {
        Profile::Attended => "run_queue_psi_attended",
        Profile::Unattended => "run_queue_psi_unattended",
    })
}
