mod capacity_host;

use capacity_host::{Host, Wrapper};
use std::process::Stdio;
use std::time::{Duration, Instant};

fn holder(host: &Host, then: &str) -> Wrapper {
    let script = format!(
        "import {{ Database }} from 'bun:sqlite'; import {{ writeFileSync }} from 'node:fs'; const lock = new Database(process.env.LOCK); lock.exec('BEGIN EXCLUSIVE'); writeFileSync(process.env.HOLDING, ''); {then}"
    );
    let mut command = host.command("bun");
    command.args(["-e", &script]).env("LOCK", host.path("agent-capacity-v1/admission.lock")).env("HOLDING", host.path("holding"));
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let wrapper = Wrapper(command.spawn().unwrap());
    host.until("the holder to take the admission lock", |host| host.path("holding").exists());
    wrapper
}

fn probe(host: &Host) -> Wrapper {
    let mut command = host.helper(&["probe", "--class", "agent"]);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
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
    let host = Host::new("capacity-admission-lock-", &[]);
    std::fs::create_dir_all(host.path("agent-capacity-v1/leases")).unwrap();
    let started = Instant::now();
    let mut first = holder(&host, "Bun.sleepSync(3000); lock.exec('COMMIT');");
    let mut second = probe(&host);
    let (first_ok, first_stderr) = finish(&host, "the holder", &mut first);
    let (second_ok, second_stderr) = finish(&host, "the waiting admission", &mut second);
    assert!(first_ok, "{first_stderr}");
    assert!(second_ok, "{second_stderr}");
    assert!(started.elapsed() >= Duration::from_secs(3), "the admission ran while the holder still held the lock");
}

#[test]
fn dead_holder_lock_is_broken() {
    let host = Host::new("capacity-admission-lock-", &[]);
    std::fs::create_dir_all(host.path("agent-capacity-v1/leases")).unwrap();
    let mut first = holder(&host, "Bun.sleepSync(60000);");
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    let mut second = probe(&host);
    let (second_ok, second_stderr) = finish(&host, "the admission after the holder died", &mut second);
    assert!(second_ok, "{second_stderr}");
}
