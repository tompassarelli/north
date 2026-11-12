use super::landing_run;
use super::request::Request;

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

#[test]
fn a_land_check_is_charged_and_recorded_as_the_check_it_runs_and_keeps_the_heavy_charge_without_a_day_of_its_runs() {
    let script = "/home/builder/.cache/land-offload/offload-command.ry70YQ";
    let wrapper = ["bash", "-euo", "pipefail", script].map(str::to_owned);
    let check = super::shape::script_subject(&wrapper, Some(script), |_| Some("cargo nextest run -p north-capacity-host\n".to_owned())).expect("the offloaded check");
    assert_eq!(super::policy::owner_prefix("land-check").to_owned() + " " + &super::shape::command_shape(&check), "land cargo nextest run", "on 2026-11-12 every offloaded check was recorded as land bash");
    assert_eq!(super::shape::script_subject(&wrapper, Some("/tmp/other"), |_| Some("true".to_owned())), None);
    let sized = |cap: Option<f64>, runs: &[super::policy::ChargedRun]| {
        let mut request = Request { name: "heavy".to_owned(), resources: super::policy::resource_class("heavy", 16.0), landing: landing_run("run", "land-check", "heavy", None, None), ..Request::default() };
        super::size_landing(&mut request, 16.0, cap, 8.0, |heavy| super::policy::landing_charge(runs, heavy, 300.0));
        assert_eq!(request.declared.unwrap().cpus, request.resources().cpus, "a check's scope is held to its charge");
        request.resources().cpus
    };
    let run = |wall_seconds: f64, cpu_seconds: f64| super::policy::ChargedRun { wall_seconds, cpu_seconds, charged_cpus: 8.0, passed: true };
    assert_eq!(sized(None, &[run(40.0, 12.0); 6]), 1.0, "on 2026-11-12 a check using 0.3 cores was charged 8");
    assert_eq!(sized(None, &[]), 8.0);
    assert_eq!(sized(Some(4.0), &[]), 4.0, "on 2026-11-12 a land check charged 4 CPUs ran in a 16-CPU scope");
    assert_eq!(sized(None, &[run(40.0, 12.0); 4]), 8.0);
    assert_eq!(sized(Some(1.0), &[run(160.0, 1200.0); 6]), 7.0, "a caller's share never charges less than its command's records need");
}
