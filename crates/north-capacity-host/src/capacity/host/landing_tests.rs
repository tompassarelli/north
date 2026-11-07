use super::landing_run;
use super::request::Request;

// KEEP(bug): land checks are charged their learned cores and slot share, not the ceiling, so three fit a 20-core host and none is held to its share; regressions of 1a9885c72 and 3f2f3437e
#[test]
fn three_land_checks_charged_their_learned_cores_fit_a_twenty_core_host_that_one_ceiling_charge_filled() {
    let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 20.0), landing: landing_run("run", "gate:session-1", "heavy", Some("abc"), Some("1")), ..Request::default() };
    super::size_landing(&mut request, 20.0, Some(4.0), || 4.81);
    assert!(3.0 * request.resources().cpus <= 16.0);
    assert!(2.0 * request.declared.unwrap().cpus > 16.0);
}

// KEEP(bug): a land check whose wrapper carries no SAFE_PUSH_LAND_HEAD (the lander's wrapped checks, a check shipped to another host) queued as an ordinary heavy lease behind the land-check claim its own lander renewed, so idle gex45 admitted nothing on 2026-11-07
#[test]
fn a_land_check_lease_is_a_landing_lease_without_the_land_head_in_its_environment() {
    assert!(landing_run("run", "land-check", "heavy", None, None));
    assert!(!landing_run("run", "heavy-command-guard", "heavy", Some("abc"), Some("1")));
}

// KEEP(bug): on 2026-11-08 gate:lander runs queued as ordinary heavy runs behind worker gates for up to 197 s, and land-offload runs up to 139 s on gex45, because only land-check and a gate carrying SAFE_PUSH_LAND_HEAD admitted as the critical class that passes the queue (north#842)
#[test]
fn a_queued_lander_run_admits_as_the_critical_class_that_starts_before_earlier_queued_worker_gates() {
    let admitted = |owner: &str| Request { name: "heavy".to_owned(), landing: landing_run("run", owner, "heavy", None, None), ..Request::default() }.admitted_class().to_owned();
    for lander in ["land-check", "land-offload", "gate:lander", "gate:lander-2", "gate:lander-probe-1", "gate:lander-replay", "gate:lander-goals"] {
        assert_eq!(admitted(lander), "critical", "{lander} waits in the worker line");
    }
    for worker in ["gate:north-842-serve", "gate:tom", "gate:lander-lead", "gate:lander-probe", "gate:landerx", "heavy-command-guard", "makgora-goals-perf"] {
        assert_eq!(admitted(worker), "heavy", "{worker} takes the landing priority");
    }
    assert!(!landing_run("estimate", "gate:lander", "heavy", None, None));
}
