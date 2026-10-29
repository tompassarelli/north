use super::admission::{Class, Facts};

pub(super) fn overloaded(facts: &Facts) -> bool {
    let cores = f64::from(facts.cores);
    let claim = match facts.class {
        Class::Heavy => facts.requested_cpus.min(cores / 2.0),
        Class::Exclusive => 0.0,
        _ => return false,
    };
    facts.run_queue_load.is_some_and(|load| load + claim > cores)
}
