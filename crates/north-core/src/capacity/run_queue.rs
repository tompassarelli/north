use super::admission::{Class, Facts, Profile, default_mib};

pub(super) fn overloaded(facts: &Facts) -> bool {
    let cores = f64::from(facts.cores);
    let claim = match facts.class {
        Class::Heavy => facts.requested_cpus.min(cores / 2.0),
        Class::Exclusive => 0.0,
        _ => return false,
    };
    if facts.cpu_pressure_measured {
        let floor = match facts.class {
            Class::Heavy => super::pressure_target::band(facts).high_basis_points,
            _ => exclusive_floor_percent(facts.profile) * 100.0,
        };
        return f64::from(facts.cpu_some_avg10_basis_points) >= floor;
    }
    facts.run_queue_load.is_some_and(|load| load + claim > cores)
}

fn exclusive_floor_percent(profile: Profile) -> f64 {
    default_mib(match profile {
        Profile::Attended => "run_queue_psi_attended",
        Profile::Unattended => "run_queue_psi_unattended",
    })
}
