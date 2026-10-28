mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::process::{Command, Stdio};

fn attended_host_with_clients(prefixes: &[&str]) -> Host {
    let host = Host::new("capacity-native-clients-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("agent-capacity-v1/mode"), "present\n").unwrap();
    fs::create_dir_all(host.path("proc")).unwrap();
    for (index, prefix) in prefixes.iter().enumerate() {
        fake_client(&host, 100 + index as u32, prefix);
    }
    host
}

fn fake_client(host: &Host, pid: u32, prefix: &str) {
    let directory = host.path(&format!("proc/{pid}"));
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("comm"), "Warcraft III.exe\n").unwrap();
    fs::write(directory.join("environ"), format!("HOME=/home/fixture\0STEAM_COMPAT_DATA_PATH={prefix}\0")).unwrap();
}

fn helper(host: &Host, arguments: &[&str]) -> Command {
    let mut command = host.helper(arguments);
    command.env_remove("AGENT_CAPACITY_GPU_CLIENTS").env("AGENT_CAPACITY_PROC", host.path("proc"));
    command
}

fn client_start(host: &Host, owner: &str, new_clients: &str, command: &[&str]) -> Wrapper {
    let arguments = [&["session", "--class", "native", "--memory-gib", "1", "--new-clients", new_clients, "--wait-seconds", "60", "--owner", owner, "--"][..], command].concat();
    Wrapper(helper(host, &arguments).stdout(Stdio::null()).spawn().unwrap())
}

fn probe(host: &Host, new_clients: &str) -> Value {
    let output = helper(host, &["probe", "--class", "native", "--memory-gib", "1", "--new-clients", new_clients]).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn owners<'a>(entries: &'a Value, class: &str) -> Vec<&'a str> {
    entries.as_array().unwrap().iter().filter(|entry| entry["class"] == class).map(|entry| entry["owner"].as_str().unwrap()).collect()
}

#[test]
fn a_prefix_running_two_warcraft_processes_counts_as_one_client() {
    let host = attended_host_with_clients(&["/prefix/pair-0", "/prefix/pair-2", "/prefix/pair-6"]);
    fake_client(&host, 200, "/prefix/pair-0");
    fake_client(&host, 201, "/prefix/pair-0");
    assert_eq!(probe(&host, "1")["gpuClients"], 3);
    fs::remove_dir_all(host.path("proc/102")).unwrap();
    assert_eq!(probe(&host, "1")["gpuClients"], 2);
}

#[test]
fn a_waiter_whose_clients_do_not_fit_lets_a_smaller_start_behind_it_take_the_free_slot() {
    let host = attended_host_with_clients(&["/prefix/a", "/prefix/b", "/prefix/c"]);
    let _wide = client_start(&host, "pair-wide", "2", &["sleep", "60"]);
    host.until("the wide start to queue on client slots", |host| host.helper_stderr().contains(r#""reason":"DEFER_GPU_CLIENTS""#));
    let started = host.path("single-started");
    let _single = client_start(&host, "pair-single", "1", &["touch", started.to_str().unwrap()]);
    host.until("the single client to start past the wide waiter", |_| started.exists());
    assert_eq!(owners(&host.status()["queued"], "native"), ["pair-wide"]);
    host.until("the single client to release its lease", |host| host.leases("native") == 0);

    fs::remove_dir_all(host.path("proc/100")).unwrap();
    host.until("the wide start to take the freed slot", |host| host.leases("native") == 1);
    assert_eq!(owners(&host.status()["holding"], "native"), ["pair-wide"]);
    assert_eq!(owners(&host.status()["queued"], "native"), Vec::<&str>::new());
}
