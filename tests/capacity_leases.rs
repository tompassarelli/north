mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::{Value, json};
use std::fs;
use std::process::Stdio;

fn host() -> Host {
    Host::new("capacity-leases-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)])
}

fn run(host: &Host, class: &str, owner: &str, timeout: &str, command: &[&str]) -> Wrapper {
    let arguments = [&["run", "--class", class, "--owner", owner, "--timeout-seconds", timeout, "--"][..], command].concat();
    Wrapper(host.helper(&arguments).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap())
}

fn decisions(stderr: &str) -> Vec<Value> {
    stderr.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect()
}

fn succeeds(host: &Host, what: &str, wrapper: &mut Wrapper) {
    let mut status = None;
    host.until(what, |_| {
        status = wrapper.exited();
        status.is_some()
    });
    assert!(status.unwrap().success(), "{what}: {}", wrapper.stderr());
}

#[test]
fn child_lease_of_a_held_lease_runs_nested_while_an_exclusive_waits() {
    let host = host();
    let (go, done) = (host.path("go"), host.path("done"));
    let child = format!(
        "while [ ! -e {go} ]; do sleep 0.05; done; exec bun {bundle} run --class heavy --owner fixture:/child --timeout-seconds 3600 -- touch {done}",
        go = go.display(),
        bundle = Host::bundle().display(),
        done = done.display(),
    );
    let mut parent = run(&host, "heavy", "fixture:/parent", "30", &["sh", "-c", &child]);
    host.until("the parent lease", |host| host.leases("heavy") == 1);
    let mut exclusive = run(&host, "exclusive", "fixture:/exclusive", "60", &["true"]);
    host.until("the exclusive ticket", |host| host.queued("exclusive") == 1);
    fs::write(&go, "").unwrap();
    host.until("the nested child", |_| done.exists());
    succeeds(&host, "the parent", &mut parent);
    succeeds(&host, "the exclusive", &mut exclusive);
    let lines = decisions(&parent.stderr());
    let lease = lines.iter().find(|line| line["decision"] == "RESERVED").unwrap()["lease"].clone();
    let nested = lines.iter().find(|line| line["decision"] == "NESTED").expect("a NESTED line");
    assert_eq!(nested["parent"], lease, "{nested}");
    assert_eq!(host.leases("heavy"), 0);
}

#[test]
fn lease_past_its_timeout_has_its_scope_killed_and_is_released() {
    let host = host();
    let pid = host.path("pid");
    let script = format!("read host _ </proc/self/stat; echo $host >{}; exec sleep 60", pid.display());
    let mut overdue = run(&host, "moderate", "fixture:/overdue", "1", &["sh", "-c", &script]);
    host.until("the overdue lease", |host| host.leases("moderate") == 1 && fs::read_to_string(&pid).is_ok_and(|text| text.ends_with('\n')));
    let sleeper: u32 = fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
    host.until("the reaped lease", |host| host.status()["holding"] == json!([]));
    let mut status = None;
    host.until("the wrapper", |_| {
        status = overdue.exited();
        status.is_some()
    });
    assert!(!fs::exists(format!("/proc/{sleeper}")).unwrap(), "the overdue scope still runs");
    assert_eq!(decisions(&overdue.stderr()).last().unwrap()["decision"], "RECLAIMED");
}

#[test]
fn waiting_exclusive_admits_a_heavy_job_whose_deadline_ends_before_the_drain() {
    let host = host();
    let release = host.path("release");
    let hold = format!("while [ ! -e {} ]; do sleep 0.05; done", release.display());
    let mut holder = run(&host, "heavy", "fixture:/holder", "600", &["sh", "-c", &hold]);
    host.until("the holder lease", |host| host.leases("heavy") == 1);
    let mut exclusive = run(&host, "exclusive", "fixture:/exclusive", "60", &["true"]);
    host.until("the exclusive ticket", |host| host.queued("exclusive") == 1);
    let mut long = run(&host, "heavy", "fixture:/long", "900", &["true"]);
    host.until("the long ticket", |host| host.queued("heavy") == 1);

    let mut short = run(&host, "heavy", "fixture:/short", "60", &["true"]);
    succeeds(&host, "the short job", &mut short);
    assert!(exclusive.exited().is_none() && long.exited().is_none());
    let queued = host.status()["queued"].clone();
    assert_eq!(queued[0]["class"], "exclusive", "{queued}");
    assert_eq!(queued[0]["waitingFor"][0]["owner"], "fixture:/holder", "{queued}");
    assert_eq!(queued[1]["class"], "heavy", "{queued}");

    fs::write(&release, "").unwrap();
    succeeds(&host, "the holder", &mut holder);
    succeeds(&host, "the exclusive", &mut exclusive);
    succeeds(&host, "the long job", &mut long);
    assert_eq!(decisions(&long.stderr())[0]["reason"], "DEFER_EXCLUSIVE_QUEUED");
}

#[test]
fn lease_whose_wrapper_pid_the_host_cannot_resolve_is_reclaimed_once_its_scope_is_gone() {
    let host = host();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let id = "60e7e7e1-9443-44ba-9fb9-eff794cca57f";
    let lease = json!({
        "schema": "agent-capacity-lease/v1", "id": id, "kind": "run", "class": "heavy", "aggregateSlice": "agent-capacity.slice",
        "owner": "smashcraft:inputs-derive", "cpus": 6.01, "memoryMiB": 9834, "createdAt": now - 600_000,
        "expiresAt": now + 3_000_000, "wrapperPid": 839, "wrapperStart": null,
    });
    fs::create_dir_all(host.path("agent-capacity-v1/leases")).unwrap();
    fs::write(host.path(&format!("agent-capacity-v1/leases/{id}.json")), format!("{lease}\n")).unwrap();
    let status = host.status();
    assert_eq!(status["holding"], json!([]), "{status}");
    assert_eq!(status["reclaimed"], 1, "{status}");
}

#[test]
fn gpu_lease_commits_at_most_its_scope_quota() {
    let host = host();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let wrapper_pid: u32 = fs::canonicalize("/proc/self").unwrap().file_name().unwrap().to_str().unwrap().parse().unwrap();
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let start = stat.rsplit_once(") ").unwrap().1.split(' ').nth(19).unwrap().to_owned();
    fs::create_dir_all(host.path("agent-capacity-v1/leases")).unwrap();
    for (id, cores) in [("4a1e2b0c-35a1-4c1e-9e0e-8f1d2c3b4a51", 0.96), ("4a1e2b0c-35a1-4c1e-9e0e-8f1d2c3b4a52", 1.01)] {
        let lease = json!({
            "schema": "agent-capacity-lease/v1", "id": id, "kind": "run", "class": "gpu", "aggregateSlice": "agent-capacity.slice",
            "owner": "wisp-verdicts-35", "cpus": 1, "memoryMiB": 2048, "declared": { "cpus": 1, "memoryMiB": 2048 },
            "estimate": { "cores": { "p50": 6, "p90": 6 } }, "createdAt": now - 60_000, "expiresAt": now + 3_000_000,
            "wrapperPid": wrapper_pid, "wrapperStart": start,
        });
        fs::write(host.path(&format!("agent-capacity-v1/leases/{id}.json")), format!("{lease}\n")).unwrap();
        fs::write(host.path(&format!("agent-capacity-v1/leases/{id}.usage")), json!({ "at": now, "liveCores": cores }).to_string()).unwrap();
    }
    let status = host.status();
    let holding = status["holding"].as_array().unwrap();
    assert_eq!(holding.len(), 2, "{status}");
    for holder in holding {
        assert!(holder["committedCpus"].as_f64().unwrap() <= 1.0, "a gpu lease committed above its 1-CPU scope quota: {holder}");
    }
    assert!(status["committedBatchCpus"].as_f64().unwrap() <= 2.0, "{status}");
}
