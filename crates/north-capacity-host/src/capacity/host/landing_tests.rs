use super::landing_run;
use super::request::Request;

#[test]
fn three_land_checks_fit_a_twenty_core_host_and_each_scope_is_held_to_its_charge() {
    let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 20.0), landing: landing_run("run", "gate:session-1", "heavy", Some("abc"), Some("1")), ..Request::default() };
    super::size_landing(&mut request, 20.0, Some(4.0), || 4.81);
    assert!(3.0 * request.resources().cpus <= 16.0);
    assert_eq!(request.declared.unwrap().cpus, request.resources().cpus, "on 2026-11-12 a land check charged 4 CPUs ran in a 16-CPU scope");
}

#[test]
fn a_land_check_lease_is_a_landing_lease_without_the_land_head_in_its_environment() {
    assert!(landing_run("run", "land-check", "heavy", None, None));
    assert!(!landing_run("run", "heavy-command-guard", "heavy", Some("abc"), Some("1")));
}

#[test]
fn a_queued_lander_run_admits_as_the_critical_class_that_starts_before_earlier_queued_worker_gates() {
    let admitted = |owner: &str| Request { name: "heavy".to_owned(), landing: landing_run("run", owner, "heavy", None, None), ..Request::default() }.admitted_class().to_owned();
    for lander in ["land-check", "land-offload", "gate:lander", "gate:lander-2", "gate:lander-probe-1", "gate:lander-replay", "gate:lander-replay-2", "gate:lander-goals"] {
        assert_eq!(admitted(lander), "critical", "{lander} waits in the worker line");
    }
    for worker in ["gate:north-842-serve", "gate:tom", "gate:lander-lead", "gate:lander-probe", "gate:landerx", "heavy-command-guard", "makgora-goals-perf"] {
        assert_eq!(admitted(worker), "heavy", "{worker} takes the landing priority");
    }
    assert!(!landing_run("estimate", "gate:lander", "heavy", None, None));
}
