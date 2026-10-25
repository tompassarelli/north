mod capacity_host;

use capacity_host::{Host, Wrapper};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

fn host(first: &str) -> Host {
    let host = Host::new("capacity-admission-lock-", &[]);
    let shim = format!(
        "#!/bin/sh\necho enter >>{log}\nif mkdir {first_dir} 2>/dev/null; then : >{holding}; {first}; fi\n{north} \"$@\"\nstatus=$?\necho leave >>{log}\nexit $status\n",
        log = host.path("admissions").display(),
        first_dir = host.path("first").display(),
        holding = host.path("holding").display(),
        north = env!("CARGO_BIN_EXE_north"),
    );
    fs::write(host.path("bin/north"), shim).unwrap();
    fs::set_permissions(host.path("bin/north"), fs::Permissions::from_mode(0o755)).unwrap();
    host
}

fn probe(host: &Host) -> Wrapper {
    let mut command = host.helper(&["probe", "--class", "agent"]);
    command.env("AGENT_CAPACITY_NORTH", host.path("bin/north")).stdout(Stdio::null()).stderr(Stdio::piped());
    Wrapper(command.spawn().unwrap())
}

fn finish(host: &Host, what: &str, wrapper: &mut Wrapper) -> (bool, String) {
    let mut status = None;
    host.until(what, |_| {
        status = wrapper.exited();
        status.is_some()
    });
    (status.unwrap().success(), wrapper.stderr())
}

#[test]
fn second_admission_waits_for_a_live_holder_past_the_old_stale_age() {
    let host = host("sleep 3");
    let mut first = probe(&host);
    host.until("the first admission to hold the lock", |host| host.path("holding").exists());
    let mut second = probe(&host);
    let (first_ok, first_stderr) = finish(&host, "the first admission", &mut first);
    let (second_ok, second_stderr) = finish(&host, "the second admission", &mut second);
    assert!(first_ok, "{first_stderr}");
    assert!(second_ok, "{second_stderr}");
    assert_eq!(fs::read_to_string(host.path("admissions")).unwrap(), "enter\nleave\nenter\nleave\n");
}

#[test]
fn dead_holder_lock_is_broken() {
    let host = host("exit 1");
    let mut first = probe(&host);
    let (first_ok, first_stderr) = finish(&host, "the first admission to die holding the lock", &mut first);
    assert!(!first_ok && first_stderr.contains("north capacity admission failed"), "{first_stderr}");
    let mut second = probe(&host);
    let (second_ok, second_stderr) = finish(&host, "the second admission", &mut second);
    assert!(second_ok, "{second_stderr}");
    assert_eq!(fs::read_to_string(host.path("admissions")).unwrap(), "enter\nenter\nleave\n");
}
