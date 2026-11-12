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
    let script = "/home/builder/.cache/land-offload/offload-command.ry70YQ/command";
    let wrapper = ["bash", "-euo", "pipefail", script].map(str::to_owned);
    let check = super::shape::script_subject(&wrapper, Some(script), |_| Some("cargo nextest run -p north-capacity-host\n".to_owned())).expect("the offloaded check");
    assert_eq!(super::policy::owner_prefix("land-check").to_owned() + " " + &super::shape::command_shape(&check), "land cargo nextest run", "on 2026-11-12 every offloaded check was recorded as land bash");
    assert_eq!(super::shape::script_subject(&wrapper, Some("/tmp/other"), |_| Some("true".to_owned())), None);
    for named in ["/home/builder/notes", "/home/builder/.cache/land-offload/offload-command.ry70YQ", "/tmp/offload-command./command", "offload-command.ry70YQ/command"] {
        let wrapper = ["bash", "-euo", "pipefail", named].map(str::to_owned);
        assert_eq!(super::shape::script_subject(&wrapper, Some(named), |_| panic!("{named} was read")), None, "a file the caller named became the run's recorded command");
    }
    let (login, session, broker) = ("0::/user.slice/user-1000.slice/session-4.scope\n", "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-77c49ab1.scope\n", "0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-worker-outside-7.service\n");
    assert!(super::lander_lease("land-check", Some("north-land"), Some(login)));
    for (owner, offload_owner, cgroup) in [("land-check", None, Some(login)), ("land-check", Some(""), Some(login)), ("land-check", Some("north-land"), Some(session)), ("land-check", Some("north-land"), Some(broker)), ("land-check", Some("north-land"), None), ("gate:tom", Some("north-land"), Some(login))] {
        assert!(!super::lander_lease(owner, offload_owner, cgroup), "the owner word {owner} made a run count toward a landing charge ({offload_owner:?}, {cgroup:?})");
    }
    let row = |at: &str, host: &str, command: &str| format!(r#"{{"at":"{at}","owner":"land-check","host":"{host}","command":["bash","-c","{command}"],"wallSeconds":40,"cpuSeconds":12,"chargedCpus":8,"exitCode":0}}"#);
    let rows = [row("2026-11-12T10:00:00.000Z", "build-1", &check[2]), row("2026-11-12T10:00:00.000Z", "build-1", "true"), row("2026-11-10T10:00:00.000Z", "build-1", &check[2]), row("2026-11-12T10:00:00.000Z", "build-2", &check[2])].join("\n");
    assert_eq!(super::sizing::charged_runs(&rows, &check, "2026-11-11T10:00:00.000Z", "build-1").len(), 1, "only the day's runs of this command on this host set its charge");
    let scan = |paths: &str, scan: &str| format!("./bin/head-north --code {paths} land scan {scan}");
    let lanes = [row("2026-11-12T10:00:00.000Z", "build-1", &scan("crates/a/,src/cli/", "leftovers")), row("2026-11-12T10:00:00.000Z", "build-1", &scan("crates/b/", "leftovers")), row("2026-11-12T10:00:00.000Z", "build-1", &scan("crates/b/", "turn-scope"))].join("\n");
    let leftovers = ["bash".to_owned(), "-c".to_owned(), scan("crates/c/", "leftovers")];
    assert_eq!(super::sizing::charged_runs(&lanes, &leftovers, "2026-11-11T10:00:00.000Z", "build-1").len(), 2, "on 2026-11-12 gex45's 124 land-check runs were 49 commands told apart by each lane's --code paths, so 4 reached the runs that lower a charge");
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
