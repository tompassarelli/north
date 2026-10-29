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

fn fake_process(host: &Host, pid: u32, comm: &str, command_line: &str, cgroup: &str, environment: &[&str]) {
    let directory = host.path(&format!("proc/{pid}"));
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("comm"), format!("{comm}\n")).unwrap();
    fs::write(directory.join("cmdline"), format!("{command_line}\0")).unwrap();
    fs::write(directory.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
    fs::write(directory.join("environ"), environment.iter().map(|entry| format!("{entry}\0")).collect::<String>()).unwrap();
}

#[test]
fn clients_in_lease_scopes_and_private_desktops_count_against_the_native_limit() {
    let host = attended_host_with_clients(&[]);
    let lease = |id: &str| format!("/user.slice/user-1000.slice/user@1000.service/native.slice/agent-capacity-{id}.scope");
    let desktop = "PRIVATE_DESKTOP_RUN=/run/user/1000/private-desktop.TFxebN";
    let executable = r"C:\Program Files (x86)\Warcraft III\_retail_\x86_64\Warcraft III.exe";
    fake_process(&host, 100, "Warcraft III.ex", executable, &lease("a"), &["STEAM_COMPAT_DATA_PATH=/prefix/lan0a", desktop]);
    fake_process(&host, 101, "Warcraft III.ex", executable, &lease("b"), &["STEAM_COMPAT_DATA_PATH=/prefix/lan0b"]);
    fake_process(&host, 102, "Warcraft III.ex", executable, "/agent.slice/private-desktop-client-1.scope", &["STEAM_COMPAT_DATA_PATH=/prefix/lan1a"]);
    fake_process(&host, 103, "steam.exe", &format!(r"c:\windows\system32\steam.exe {executable} -launch"), &lease("c"), &["STEAM_COMPAT_DATA_PATH=/prefix/lan1b", desktop]);
    fake_process(&host, 104, "bash", "bash -c Warcraft III.exe", "/user.slice/session.scope", &["STEAM_COMPAT_DATA_PATH=/prefix/lan1b"]);

    let at_limit = probe(&host, "1");
    assert_eq!(at_limit["gpuClients"], 4);
    assert_eq!(at_limit["reason"], "DEFER_GPU_CLIENTS");

    let output = host.helper(&["status"]).env_remove("AGENT_CAPACITY_GPU_CLIENTS").env("AGENT_CAPACITY_PROC", host.path("proc")).output().unwrap();
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    let clients = status["warcraftClients"].as_array().unwrap();
    let state = |prefix: &str| clients.iter().find(|client| client["prefix"] == format!("prefix:{prefix}")).map(|client| (client["state"].as_str().unwrap().to_owned(), client["privateDesktop"].as_bool().unwrap()));
    assert_eq!(clients.len(), 4);
    assert_eq!(state("/prefix/lan0a"), Some(("running".to_owned(), true)));
    assert_eq!(state("/prefix/lan0b"), Some(("running".to_owned(), false)));
    assert_eq!(state("/prefix/lan1a"), Some(("running".to_owned(), true)));
    assert_eq!(state("/prefix/lan1b"), Some(("launching".to_owned(), true)));

    fs::remove_dir_all(host.path("proc/103")).unwrap();
    assert_eq!(probe(&host, "1")["reason"], "RUN");
}
