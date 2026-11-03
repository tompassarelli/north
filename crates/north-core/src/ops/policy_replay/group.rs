use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitOptions, WaitidOptions, kill_process_group, set_child_subreaper, waitid, waitpgid};

static LIVE: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static STOPPING: AtomicBool = AtomicBool::new(false);

pub struct TimedOut;

fn live() -> std::sync::MutexGuard<'static, Vec<u32>> {
    LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn group_of(pid: u32) -> Option<Pid> {
    Pid::from_raw(pid as i32)
}

fn kill_group(pid: u32) {
    if let Some(group) = group_of(pid) {
        let _ = kill_process_group(group, Signal::Kill);
    }
}

pub fn start_group(command: &mut Command) -> std::io::Result<Child> {
    let mut groups = live();
    if STOPPING.load(Ordering::SeqCst) {
        return Err(std::io::Error::other("replay is stopping"));
    }
    set_child_subreaper(Pid::from_raw(std::process::id() as i32)).map_err(std::io::Error::from)?;
    let child = command.process_group(0).spawn()?;
    groups.push(child.id());
    Ok(child)
}

pub fn stop_group(child: &mut Child) -> Option<ExitStatus> {
    let mut groups = live();
    let at = groups.iter().position(|pid| *pid == child.id())?;
    kill_group(child.id());
    let status = child.wait().ok();
    if let Some(group) = group_of(child.id()) {
        while !matches!(waitpgid(group, WaitOptions::empty()), Err(Errno::CHILD)) {}
    }
    groups.remove(at);
    status
}

pub fn exit_code(status: Option<ExitStatus>) -> i64 {
    use std::os::unix::process::ExitStatusExt;
    status.map_or(-1, |status| status.code().map_or_else(|| -i64::from(status.signal().unwrap_or(0)), i64::from))
}

fn exited(child: &Child) -> bool {
    let Some(pid) = group_of(child.id()) else { return true };
    matches!(waitid(WaitId::Pid(pid), WaitidOptions::EXITED | WaitidOptions::NOWAIT | WaitidOptions::NOHANG), Ok(Some(_)) | Err(Errno::CHILD))
}

pub fn wait_group(child: &mut Child, timeout: Duration) -> Result<i64, TimedOut> {
    let deadline = Instant::now() + timeout;
    loop {
        if exited(child) {
            return Ok(exit_code(stop_group(child)));
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(TimedOut);
        }
        std::thread::sleep(Duration::from_millis(10).min(deadline - now));
    }
}

pub fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(Duration::MAX)
}

pub fn run_check(command: &str, cwd: &Path, timeout: f64) -> bool {
    let mut shell = Command::new("bash");
    shell.args(["-c", command]).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut child) = start_group(&mut shell) else { return false };
    match wait_group(&mut child, seconds(timeout)) {
        Ok(code) => code == 0,
        Err(TimedOut) => {
            stop_group(&mut child);
            false
        }
    }
}

fn tagged(tag: &[u8]) -> Vec<u32> {
    let Ok(entries) = fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let environ = fs::read(entry.path().join("environ")).ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            let state = stat.rsplit_once(')')?.1.split_whitespace().next()?.to_owned();
            (environ.split(|byte| *byte == 0).any(|var| var == tag) && state != "Z").then_some(pid)
        })
        .collect()
}

pub fn reap(tag: &[u8], child: &mut Child) -> usize {
    let found = tagged(tag);
    for (signal, grace) in [(Signal::Term, 3.0), (Signal::Kill, 1.0)] {
        for pid in tagged(tag) {
            if let Some(pid) = Pid::from_raw(pid as i32) {
                let _ = rustix::process::kill_process(pid, signal);
            }
        }
        let deadline = Instant::now() + seconds(grace);
        while !tagged(tag).is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    stop_group(child);
    for pid in &found {
        if let Some(pid) = Pid::from_raw(*pid as i32) {
            let _ = rustix::process::waitpid(Some(pid), WaitOptions::empty());
        }
    }
    found.len()
}

pub fn install_signal_handlers() {
    let (ready, installed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = signal(SignalKind::terminate()).ok();
            let mut interrupt = signal(SignalKind::interrupt()).ok();
            let mut hangup = signal(SignalKind::hangup()).ok();
            let _ = ready.send(());
            let number = tokio::select! {
                () = next(&mut term) => 15,
                () = next(&mut interrupt) => 2,
                () = next(&mut hangup) => 1,
            };
            STOPPING.store(true, Ordering::SeqCst);
            let groups: Vec<u32> = live().clone();
            for pid in &groups {
                kill_group(*pid);
            }
            for pid in &groups {
                if let Some(group) = group_of(*pid) {
                    while !matches!(waitpgid(group, WaitOptions::empty()), Err(Errno::CHILD)) {}
                }
            }
            std::process::exit(128 + number);
        });
    });
    let _ = installed.recv();
}

async fn next(stream: &mut Option<tokio::signal::unix::Signal>) {
    match stream {
        Some(stream) => {
            stream.recv().await;
        }
        None => std::future::pending().await,
    }
}
