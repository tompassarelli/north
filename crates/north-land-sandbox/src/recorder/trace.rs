use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::ptrace::{self, Options};
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use super::Recorder;
use super::arch;
use super::calls::{Plan, RoleSource, Target, open_role, plan};
use super::memory::{fd_path, read_path, read_u64, resolve};
use crate::classify::Role;
use crate::not_inputs;

const OPTIONS: Options = Options::PTRACE_O_TRACESECCOMP
    .union(Options::PTRACE_O_TRACEFORK)
    .union(Options::PTRACE_O_TRACEVFORK)
    .union(Options::PTRACE_O_TRACECLONE)
    .union(Options::PTRACE_O_TRACEEXEC)
    .union(Options::PTRACE_O_EXITKILL);

const AT_EMPTY_PATH: u64 = 0x1000;
const STOPS: [Signal; 4] = [Signal::SIGSTOP, Signal::SIGTSTP, Signal::SIGTTIN, Signal::SIGTTOU];
const HELD_WAIT: Duration = Duration::from_micros(200);
const HELD_CHECK: Duration = Duration::from_millis(5);
const SIGCONT_BIT: u64 = bit(libc::SIGCONT);
const NO_EFFECT_WHILE_STOPPED: u64 = bit(libc::SIGCHLD) | bit(libc::SIGURG) | bit(libc::SIGWINCH) | bit(libc::SIGSTOP) | bit(libc::SIGTSTP) | bit(libc::SIGTTIN) | bit(libc::SIGTTOU);

const fn bit(signal: i32) -> u64 {
    1 << (signal - 1)
}

pub fn follow(recorder: &mut Recorder, root: Pid) -> Result<u8, String> {
    match waitpid(root, Some(WaitPidFlag::__WALL)) {
        Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {}
        other => return Err(format!("the child did not stop for tracing: {other:?}")),
    }
    ptrace::setoptions(root, OPTIONS).map_err(|error| format!("ptrace options: {error}"))?;
    resume(root, None);
    let mut stopped: HashSet<Pid> = HashSet::from([root]);
    let mut held: HashSet<Pid> = HashSet::new();
    let mut checked = Instant::now();
    loop {
        let flags = if held.is_empty() { WaitPidFlag::__WALL } else { WaitPidFlag::__WALL | WaitPidFlag::WNOHANG };
        let status = match waitpid(None, Some(flags)) {
            Ok(WaitStatus::StillAlive) => {
                std::thread::sleep(HELD_WAIT);
                if checked.elapsed() >= HELD_CHECK {
                    held.retain(|pid| !release(*pid));
                    checked = Instant::now();
                }
                continue;
            }
            Ok(status) => status,
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(format!("wait for the traced tree: {error}")),
        };
        match status {
            WaitStatus::Exited(pid, code) if pid == root => return Ok(u8::try_from(code & 0xff).unwrap_or(1)),
            WaitStatus::Signaled(pid, signal, _) if pid == root => return Ok(u8::try_from(128 + signal as i32).unwrap_or(1)),
            WaitStatus::Exited(pid, _) | WaitStatus::Signaled(pid, _, _) => {
                stopped.remove(&pid);
                held.remove(&pid);
            }
            WaitStatus::PtraceEvent(pid, _, event) => {
                if event == libc::PTRACE_EVENT_SECCOMP {
                    observe(recorder, pid);
                }
                resume(pid, None);
            }
            WaitStatus::Stopped(pid, signal) => {
                let first = stopped.insert(pid);
                if STOPS.contains(&signal) && !(first && signal == Signal::SIGSTOP) {
                    held.insert(pid);
                    continue;
                }
                let delivered = if signal == Signal::SIGTRAP || (first && signal == Signal::SIGSTOP) { None } else { Some(signal) };
                resume(pid, delivered);
            }
            _ => {}
        }
    }
}

fn release(pid: Pid) -> bool {
    let wakes = std::fs::read_to_string(format!("/proc/{pid}/status")).map_or(true, |status| wakes_a_stopped_process(&status));
    if wakes {
        resume(pid, None);
    }
    wakes
}

fn wakes_a_stopped_process(status: &str) -> bool {
    let mask = |field: &str| status.lines().filter(|line| line.starts_with(field)).filter_map(|line| line.split_whitespace().nth(1)).filter_map(|mask| u64::from_str_radix(mask, 16).ok()).fold(0, |all, mask| all | mask);
    let pending = mask("SigPnd:") | mask("ShdPnd:");
    let deferred = mask("SigBlk:") | mask("SigIgn:") | mask("SigCgt:") | NO_EFFECT_WHILE_STOPPED;
    pending & SIGCONT_BIT != 0 || pending & !deferred != 0
}

fn resume(pid: Pid, signal: Option<Signal>) {
    let _ = ptrace::cont(pid, signal);
}

fn observe(recorder: &mut Recorder, pid: Pid) {
    let call = match arch::registers(pid) {
        Ok(call) => call,
        Err(reason) => return incomplete(recorder, reason),
    };
    let name = arch::name(call.number).unwrap_or("an unlisted call");
    if let Err(reason) = record(recorder, pid, name, &call) {
        incomplete(recorder, format!("recorder could not read {name} of process {pid}: {reason}"));
    }
}

fn incomplete(recorder: &mut Recorder, reason: String) {
    recorder.mark_unreusable(reason.clone());
    recorder.mark_incomplete(reason);
}

fn record(recorder: &mut Recorder, pid: Pid, name: &str, call: &arch::Call) -> Result<(), String> {
    let process = u32::try_from(pid.as_raw()).map_err(|error| error.to_string())?;
    let io = |error: std::io::Error| error.to_string();
    match plan(name, &call.args) {
        Plan::Ambient if not_inputs::is_call(name) => {}
        Plan::Ambient => recorder.mark_unreusable(format!("ambient call {name}")),
        Plan::Listing(fd) => recorder.observe(process, Role::Listing, fd_path(process, fd).map_err(io)?),
        Plan::Paths(targets) => {
            for Target { dirfd, pointer, role, fd_flags } in targets {
                let role = match role {
                    RoleSource::Fixed(role) => role,
                    RoleSource::Flags(flags) => open_role(flags),
                    RoleSource::How(address) => open_role(read_u64(process, address).map_err(io)?),
                };
                let raw = if pointer == 0 { PathBuf::new() } else { read_path(process, pointer).map_err(io)? };
                let on_fd = fd_flags.is_some_and(|flags| flags & AT_EMPTY_PATH != 0);
                if let Some(path) = resolve(process, dirfd, &raw, on_fd).map_err(io)? {
                    recorder.observe(process, role, path);
                }
            }
        }
    }
    Ok(())
}
