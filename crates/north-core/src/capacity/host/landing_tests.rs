use super::landing_run;
use super::request::Request;

fn admitted(owner: &str, land_head: Option<&str>, landing: Option<&str>) -> String {
    let request = Request { name: "heavy".to_owned(), landing: landing_run("run", owner, "heavy", land_head, landing), ..Request::default() };
    request.admitted_class().to_owned()
}

#[test]
fn a_gated_run_of_an_offloaded_land_check_is_admitted_as_landing() {
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("1")), "critical");
    assert_eq!(admitted("gate:anything", Some("abc"), Some("1")), "critical");
}

#[test]
fn a_gated_run_without_the_landing_marker_stays_plain_heavy() {
    assert_eq!(admitted("gate:session-1", Some("abc"), None), "heavy");
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("")), "heavy");
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("0")), "heavy");
}

#[test]
fn the_landing_marker_alone_or_on_another_owner_does_not_admit_as_landing() {
    assert_eq!(admitted("gate:session-1", None, Some("1")), "heavy");
    assert_eq!(admitted("gate:session-1", Some(""), Some("1")), "heavy");
    assert_eq!(admitted("worker", Some("abc"), Some("1")), "heavy");
}

#[test]
fn the_land_check_owner_is_landing_with_a_land_head() {
    assert_eq!(admitted("land-check", Some("abc"), None), "critical");
    assert_eq!(admitted("land-check", None, None), "heavy");
}

#[test]
fn only_runs_of_batch_classes_are_landing() {
    assert!(!landing_run("session", "land-check", "heavy", Some("abc"), None));
    assert!(!landing_run("run", "land-check", "light", Some("abc"), None));
}

fn landing_cpus(class: &str, owner: &str, cores: f64) -> (f64, Option<f64>) {
    let mut request = Request { name: class.to_owned(), resources: super::policy::resource_class(class, cores), landing: landing_run("run", owner, class, Some("abc"), Some("1")), ..Request::default() };
    super::size_landing(&mut request, cores, || 6.0);
    let charged = request.resources().cpus;
    (charged, super::policy::scope_cpu_quota(request.admitted_class(), charged, request.declared.map(|declared| declared.cpus)))
}

#[test]
fn a_land_check_bursts_to_the_host_less_the_reserve_and_is_charged_its_land_check_cores() {
    assert_eq!(landing_cpus("moderate", "land-check", 20.0), (6.0, Some(16.0)));
    assert_eq!(landing_cpus("heavy", "gate:session-1", 20.0), (6.0, Some(16.0)));
    assert_eq!(landing_cpus("moderate", "land-check", 24.0), (6.0, Some(20.0)));
}

#[test]
fn a_land_check_never_gets_fewer_cores_than_a_heavy_build() {
    assert_eq!(landing_cpus("moderate", "land-check", 9.0).0, 6.0);
    assert_eq!(landing_cpus("moderate", "land-check", 10.0).0, 6.0);
    assert_eq!(landing_cpus("moderate", "land-check", 4.0).0, 4.0);
}

#[test]
fn a_worker_build_keeps_its_class_cores() {
    assert_eq!(landing_cpus("heavy", "worker", 20.0), (6.0, Some(6.0)));
    assert_eq!(landing_cpus("moderate", "worker", 20.0), (2.0, Some(2.0)));
}

#[test]
fn a_land_check_keeps_the_memory_it_is_charged_and_declares_the_critical_ceiling() {
    let charged = super::policy::Resources { cpus: 2.0, memory_mib: 2460.0 };
    let mut request = Request { name: "moderate".to_owned(), resources: Some(charged), landing: landing_run("run", "land-check", "moderate", Some("abc"), None), ..Request::default() };
    super::size_landing(&mut request, 20.0, || 4.81);
    assert_eq!(request.resources(), super::policy::Resources { cpus: 4.81, memory_mib: 2460.0 });
    assert_eq!(request.declared, Some(super::policy::Resources { cpus: 16.0, memory_mib: 16384.0 }));
}

#[test]
fn three_land_checks_charged_their_learned_cores_fit_a_twenty_core_host_that_one_ceiling_charge_filled() {
    let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 20.0), landing: landing_run("run", "gate:session-1", "heavy", Some("abc"), Some("1")), ..Request::default() };
    super::size_landing(&mut request, 20.0, || 4.81);
    assert!(3.0 * request.resources().cpus <= 16.0);
    assert!(2.0 * request.declared.unwrap().cpus > 16.0);
}

#[test]
fn a_land_check_whose_learned_charge_exceeds_its_ceiling_is_charged_the_ceiling() {
    let mut request = Request { name: "moderate".to_owned(), resources: super::policy::resource_class("moderate", 8.0), landing: landing_run("run", "land-check", "moderate", Some("abc"), None), ..Request::default() };
    super::size_landing(&mut request, 8.0, || 12.0);
    assert_eq!(request.resources().cpus, request.declared.unwrap().cpus);
}

#[test]
fn a_run_that_is_not_a_land_check_never_reads_the_land_check_charge() {
    let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 20.0), landing: false, ..Request::default() };
    super::size_landing(&mut request, 20.0, || panic!("read the land-check charge"));
    assert_eq!(request.resources().cpus, 6.0);
}
