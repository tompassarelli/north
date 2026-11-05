#![cfg(test)]

use super::admission::{AheadTicket, Class, Facts, Profile, Reason, decide};
use super::native_hold::LeaseFact;

pub(super) use super::fixtures::facts;

pub(super) fn running(class: &str) -> LeaseFact {
    LeaseFact { id: format!("{class}-lease"), class: class.to_owned(), owner: "lane".to_owned(), quiet: false, pair: None, created_at: 0.0, memory_mib: 0.0, quiet_seconds: None, usage: None, new_clients: 0, running_clients: 0, clients_seen: 0, started_owners: Default::default() }
}

fn tickets(classes: &[Class]) -> Vec<AheadTicket> {
    classes.iter().map(|class| AheadTicket { requested_class: Some(*class), ..AheadTicket::default() }).collect()
}

fn gex45_on_2026_10_28(class: Class, disk_free_gib: f64, queued_ahead_classes: &[Class]) -> Facts {
    Facts {
        profile: Profile::Unattended,
        cores: 20,
        memory_total_mib: 64000.0,
        memory_available_mib: 54388.0,
        leased_memory_mib: 5220.0,
        leased_batch_cpus: 6.0,
        leased_native_cpus: 1.35,
        requested_cpus: if class == Class::Heavy { 6.0 } else { 2.0 },
        requested_memory_mib: if class == Class::Heavy { 8192.0 } else { 2048.0 },
        peer_batch_runs: 1,
        queued_ahead: u32::try_from(queued_ahead_classes.len()).unwrap(),
        queued_ahead_tickets: tickets(queued_ahead_classes),
        disk_free_gib: Some(disk_free_gib),
        ..facts(class)
    }
}

// KEEP(bug): moderate work passes heavy builds the disk floor holds; regression of the gex45 2026-10-28 stall (north#259, commit 07fb5bc0d)
#[test]
fn heavy_builds_held_by_the_disk_floor_do_not_queue_moderate_work_behind_them() {
    let low = 12.39;
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Heavy, low, &[])), Reason::DeferDiskFree);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, low, &[Class::Heavy, Class::Heavy])), Reason::Run);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, low, &[Class::Heavy, Class::Heavy, Class::Moderate])), Reason::DeferQueued);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, 20.0, &[Class::Heavy, Class::Heavy])), Reason::DeferQueued);
}

// KEEP(bug): an idle host reads the 1-minute load once CPU pressure is calm; regression of whiterabbit 2026-10-31 (north#380)
#[test]
fn once_cpu_pressure_is_calm_the_run_queue_is_the_one_minute_load_so_an_idle_host_admits_heavy_work() {
    let whiterabbit = Facts { class: Class::Heavy, profile: Profile::Unattended, cores: 16, leased_batch_cpus: 2.0, requested_cpus: 4.0, requested_memory_mib: 8192.0, cpu_some_avg10_basis_points: 3, ..facts(Class::Heavy) };
    let load = super::enrich::sustained_load("6.85 14.44 18.68 2/1904 88213\n", whiterabbit.cpu_some_avg10_basis_points);
    assert_eq!(load, Some(6.85));
    assert_eq!(decide(&Facts { run_queue_load: load, ..whiterabbit }), Reason::Run);
}

fn whiterabbit_on_2026_10_31(requested_cpus: f64, leased_batch_cpus: f64, ahead: &[AheadTicket]) -> Facts {
    Facts {
        cores: 16,
        leased_batch_cpus,
        cpu_some_avg10_basis_points: 68,
        requested_cpus,
        queued_ahead: u32::try_from(ahead.len()).unwrap(),
        queued_ahead_tickets: ahead.to_vec(),
        ..facts(Class::Heavy)
    }
}

// KEEP(mutant): a 24-core attended host at load 13 and 7 percent pressure admits heavy and defers it over the threshold; mutant n380-psi-ignored (f70188fed, north#380)
#[test]
fn a_24_core_attended_host_at_load_13_and_psi_7_percent_admits_heavy_and_defers_it_over_the_threshold() {
    let measured = |class, load, psi| Facts { run_queue_load: Some(load), cpu_pressure_measured: true, cpu_some_avg10_basis_points: psi, requested_cpus: 6.0, ..facts(class) };
    assert_eq!(decide(&measured(Class::Heavy, 13.0, 700)), Reason::Run);
    assert_eq!(decide(&measured(Class::Heavy, 40.0, 700)), Reason::Run);
    assert_eq!(decide(&measured(Class::Exclusive, 13.0, 700)), Reason::Run);
    assert_eq!(decide(&measured(Class::Heavy, 13.0, 4000)), Reason::DeferRunQueue);
    assert_eq!(decide(&measured(Class::Exclusive, 13.0, 4000)), Reason::DeferRunQueue);
    assert!(!super::run_queue::overloaded(&measured(Class::Heavy, 13.0, 2499)));
    assert_eq!(decide(&measured(Class::Heavy, 13.0, 2499)), Reason::Run);
    assert_eq!(decide(&measured(Class::Heavy, 13.0, 2500)), Reason::DeferRunQueue);
    assert!(!super::run_queue::overloaded(&measured(Class::Exclusive, 13.0, 3999)));
}

// KEEP(bug): a fitting request passes a heavy ticket the free share cannot fit until it waited the bound; regression of north#385 (strict FIFO held every smaller request)
#[test]
fn a_heavy_ticket_that_does_not_fit_lets_a_fitting_one_behind_it_pass_until_it_has_waited_past_the_bound() {
    let head = |queued_seconds| AheadTicket { requested_class: Some(Class::Heavy), requested_cpus: Some(6.0), queued_seconds };
    let guard = |queued_seconds| AheadTicket { requested_class: Some(Class::Heavy), requested_cpus: Some(2.0), queued_seconds };
    let bound = super::admission::default_mib("backfill_seconds");
    assert_eq!(decide(&whiterabbit_on_2026_10_31(6.0, 12.5, &[])), Reason::Run);
    assert_eq!(decide(&whiterabbit_on_2026_10_31(2.0, 7.89, &[head(21.0)])), Reason::Run);
    assert_eq!(decide(&whiterabbit_on_2026_10_31(2.0, 7.89, &[head(21.0), guard(21.0)])), Reason::DeferQueued);
    assert_eq!(decide(&whiterabbit_on_2026_10_31(2.0, 7.89, &[head(bound)])), Reason::DeferQueued);
    assert_eq!(decide(&whiterabbit_on_2026_10_31(2.0, 5.89, &[head(bound)])), Reason::DeferQueued);
    assert_eq!(decide(&whiterabbit_on_2026_10_31(6.0, 5.89, &[])), Reason::Run);
    let moderate = |ahead: &[AheadTicket]| Facts { class: Class::Moderate, ..whiterabbit_on_2026_10_31(2.0, 7.89, ahead) };
    assert_eq!(decide(&moderate(&[head(21.0)])), Reason::Run, "moderate work passes a heavy ticket the free share cannot fit");
    assert_eq!(decide(&moderate(&[head(bound)])), Reason::DeferQueued);
}

// KEEP(bug): a land check on a twenty-core host runs at critical ahead of a queued heavy build; regression of north#360 (land checks held at 2 CPUs)
#[test]
fn a_land_check_on_a_twenty_core_host_starts_ahead_of_a_queued_heavy_build_beside_running_ones() {
    let gex45 = |class| Facts {
        cores: 20,
        requested_cpus: 16.0,
        leased_batch_cpus: 12.0,
        cpu_some_avg10_basis_points: 1000,
        cpu_pressure_measured: true,
        leases: vec![running("heavy"); 2],
        queued_ahead: 1,
        queued_ahead_tickets: tickets(&[Class::Heavy]),
        ..facts(class)
    };
    assert_eq!(decide(&gex45(Class::Critical)), Reason::Run, "the land check passes the queued worker build");
    assert_eq!(decide(&Facts { requested_cpus: 6.0, ..gex45(Class::Heavy) }), Reason::DeferQueued, "the worker build behind it keeps its place");
}
