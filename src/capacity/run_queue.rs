use super::admission::{Class, Facts, aggregate_cpus, default_mib};

pub(super) fn overloaded(facts: &Facts) -> bool {
    matches!(facts.class, Class::Heavy | Class::Exclusive) && facts.run_queue_load.is_some_and(|load| load > f64::from(facts.cores)) && !within_heavy_reserve(facts)
}

fn within_heavy_reserve(facts: &Facts) -> bool {
    let committed = facts.leased_batch_cpus + facts.unleased_heavy_cores + facts.requested_cpus;
    facts.class == Class::Heavy
        && (facts.gpu_clients > 0 || facts.leased_native_cpus > 0.0)
        && committed <= default_mib("heavy_reserve_cpus")
        && committed + facts.leased_native_cpus <= aggregate_cpus(facts.profile, facts.cores)
}
