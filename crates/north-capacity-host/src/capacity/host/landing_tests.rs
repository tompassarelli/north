use super::landing_run;
use super::request::Request;

// KEEP(bug): land checks are charged their learned cores, not the ceiling, so three fit a 20-core host; regression of 1a9885c72
#[test]
fn three_land_checks_charged_their_learned_cores_fit_a_twenty_core_host_that_one_ceiling_charge_filled() {
    let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 20.0), landing: landing_run("run", "gate:session-1", "heavy", Some("abc"), Some("1")), ..Request::default() };
    super::size_landing(&mut request, 20.0, None, || 4.81);
    assert!(3.0 * request.resources().cpus <= 16.0);
    assert!(2.0 * request.declared.unwrap().cpus > 16.0);
}
