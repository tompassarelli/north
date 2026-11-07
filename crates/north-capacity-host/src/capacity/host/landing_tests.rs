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
