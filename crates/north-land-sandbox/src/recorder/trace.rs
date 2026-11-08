use std::collections::HashSet;
use std::time::Duration;

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

const OPTIONS: Options = Options::PTRACE_O_TRACESECCOMP
    .union(Options::PTRACE_O_TRACEFORK)
    .union(Options::PTRACE_O_TRACEVFORK)
    .union(Options::PTRACE_O_TRACECLONE)
    .union(Options::PTRACE_O_TRACEEXEC)
    .union(Options::PTRACE_O_EXITKILL);

const STOPS: [Signal; 4] = [Signal::SIGSTOP, Signal::SIGTSTP, Signal::SIGTTIN, Signal::SIGTTOU];
const HELD_POLL: Duration = Duration::from_millis(5);
const SIGCONT_BIT: u64 = 1 << (libc::SIGCONT - 1);

pub fn follow(recorder: &mut Recorder, root: Pid) -> Result<u8, String> {
    match waitpid(root, Some(WaitPidFlag::__WALL)) {
        Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {}
        other => return Err(format!("the child did not stop for tracing: {other:?}")),
    }
    ptrace::setoptions(root, OPTIONS).map_err(|error| format!("ptrace options: {error}"))?;
    resume(root, None);
    let mut stopped: HashSet<Pid> = HashSet::from([root]);
    let mut held: HashSet<Pid> = HashSet::new();
    loop {
        let flags = if held.is_empty() { WaitPidFlag::__WALL } else { WaitPidFlag::__WALL | WaitPidFlag::WNOHANG };
        let status = match waitpid(None, Some(flags)) {
            Ok(WaitStatus::StillAlive) => {
                std::thread::sleep(HELD_POLL);
                held.retain(|pid| !release(*pid));
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
    let continued = std::fs::read_to_string(format!("/proc/{pid}/status")).map_or(true, |status| {
        status.lines().filter(|line| line.starts_with("SigPnd:") || line.starts_with("ShdPnd:")).any(|line| line.split_whitespace().nth(1).and_then(|mask| u64::from_str_radix(mask, 16).ok()).is_some_and(|mask| mask & SIGCONT_BIT != 0))
    });
    if continued {
        resume(pid, None);
    }
    continued
}

fn resume(pid: Pid, signal: Option<Signal>) {
    let _ = ptrace::cont(pid, signal);
}

fn observe(recorder: &mut Recorder, pid: Pid) {
    let call = match arch::registers(pid) {
        Ok(call) => call,
        Err(reason) => return recorder.mark_unreusable(reason),
    };
    let name = arch::name(call.number).unwrap_or("an unlisted call");
    if let Err(reason) = record(recorder, pid, name, &call) {
        recorder.mark_unreusable(format!("recorder could not read {name} of process {pid}: {reason}"));
    }
}

fn record(recorder: &mut Recorder, pid: Pid, name: &str, call: &arch::Call) -> Result<(), String> {
    let process = u32::try_from(pid.as_raw()).map_err(|error| error.to_string())?;
    let io = |error: std::io::Error| error.to_string();
    match plan(name, &call.args) {
        Plan::Ambient => recorder.mark_unreusable(format!("ambient call {name}")),
        Plan::Listing(fd) => recorder.observe(process, Role::Listing, fd_path(process, fd).map_err(io)?),
        Plan::Paths(targets) => {
            for Target { dirfd, pointer, role } in targets {
                let role = match role {
                    RoleSource::Fixed(role) => role,
                    RoleSource::Flags(flags) => open_role(flags),
                    RoleSource::How(address) => open_role(read_u64(process, address).map_err(io)?),
                };
                let raw = read_path(process, pointer).map_err(io)?;
                if let Some(path) = resolve(process, dirfd, &raw).map_err(io)? {
                    recorder.observe(process, role, path);
                }
            }
        }
    }
    Ok(())
}
