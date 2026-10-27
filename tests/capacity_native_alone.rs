mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::process::{Command, Stdio};

fn host_with_clients(prefixes: &[&str]) -> Host {
    let host = Host::new("capacity-native-alone-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("agent-capacity-v1/mode"), "present\n").unwrap();
    for (index, prefix) in prefixes.iter().enumerate() {
        let directory = host.path(&format!("proc/{}", 100 + index));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("comm"), "Warcraft III.exe\n").unwrap();
        fs::write(directory.join("environ"), format!("HOME=/home/fixture\0STEAM_COMPAT_DATA_PATH={prefix}\0")).unwrap();
    }
    host
}

fn helper(host: &Host, arguments: &[&str]) -> Command {
    let mut command = host.helper(arguments);
    command.env_remove("AGENT_CAPACITY_GPU_CLIENTS").env("AGENT_CAPACITY_PROC", host.path("proc"));
    command
}

fn session(host: &Host, owner: &str, flags: &[&str], command: &[&str]) -> Wrapper {
    let arguments = [&["session", "--class", "native", "--memory-gib", "1", "--owner", owner][..], flags, &["--"][..], command].concat();
    Wrapper(helper(host, &arguments).stdout(Stdio::null()).spawn().unwrap())
}

fn probe(host: &Host, new_clients: &str) -> Value {
    let output = helper(host, &["probe", "--class", "native", "--memory-gib", "1", "--new-clients", new_clients]).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_queued_alone_request_holds_every_new_native_lease_until_it_has_run_and_runs_once_no_client_is_live() {
    let host = host_with_clients(&["/prefix/a", "/prefix/b"]);
    let captured = host.path("capture-ran");
    let _capture = session(&host, "sc-28-perf", &["--new-clients", "0", "--alone"], &["touch", captured.to_str().unwrap()]);
    host.until("the alone request to queue behind the live clients", |host| host.helper_stderr().contains(r#""decision":"QUEUED","reason":"DEFER_NATIVE_ALONE""#));
    assert_eq!(host.queued("native"), 1);
    assert!(!captured.exists());

    for new_clients in ["0", "1"] {
        assert_eq!(probe(&host, new_clients)["reason"], "DEFER_NATIVE_ALONE", "{new_clients} new clients");
    }
    let restarted = host.path("pair-restarted");
    let _later = session(&host, "pair-0", &["--new-clients", "1", "--wait-seconds", "60"], &["touch", restarted.to_str().unwrap()]);
    host.until("the later start to queue behind the alone request", |host| host.queued("native") == 2);
    assert!(!restarted.exists());

    fs::remove_dir_all(host.path("proc/100")).unwrap();
    fs::remove_dir_all(host.path("proc/101")).unwrap();
    host.until("the alone request to run", |_| captured.exists());
    host.until("the later start to run after it", |_| restarted.exists());
    assert_eq!(host.queued("native"), 0);
}
