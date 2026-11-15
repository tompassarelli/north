use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::broker::Source;
use crate::platform::session_scope;
use super::http::{Request, Response};

const ROTATE_BYTES: u64 = 32 << 20;

pub struct CallLog {
    path: Option<PathBuf>,
    quota: Option<PathBuf>,
}

impl CallLog {
    pub fn at(path: PathBuf, quota: PathBuf) -> Self {
        Self { path: Some(path), quota: Some(quota) }
    }

    #[cfg(test)]
    pub fn off() -> Self {
        Self { path: None, quota: None }
    }

    pub fn record(&self, caller: &str, request: &Request, response: &Response, source: Source) {
        if let Some(directory) = &self.quota {
            keep_quota(directory, request, response);
        }
        let Some(path) = &self.path else {
            return;
        };
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let line = format!("{at}\t{caller}\t{}\t{}\t{}\t{}\t{}\n", source.name(), response.status, request.method, request.target, token_kind(request));
        if std::fs::metadata(path).is_ok_and(|meta| meta.len() > ROTATE_BYTES) {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

pub struct Caller {
    pub name: String,
    pub cgroup: Option<String>,
}

impl Caller {
    pub fn in_worker_scope(&self) -> bool {
        self.cgroup.as_deref().is_some_and(|cgroup| session_scope::worker_cgroup(cgroup) && !session_scope::lane_only_cgroup(cgroup))
    }
}

pub fn caller(stream: &UnixStream) -> Caller {
    let Some((pidfd, pid)) = nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerPidfd).ok().and_then(|pidfd| pid_of(&pidfd).map(|pid| (pidfd, pid))) else {
        return Caller { name: "unknown".to_owned(), cgroup: None };
    };
    let process = PathBuf::from(format!("/proc/{pid}"));
    let cgroup = std::fs::read_to_string(process.join("cgroup")).ok();
    let name = session_of(&process).or_else(|| unit_of(&process)).unwrap_or_else(|| format!("pid-{pid}"));
    Caller { name, cgroup: cgroup.filter(|_| alive(&pidfd)) }
}

fn pid_of(pidfd: &OwnedFd) -> Option<i32> {
    let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd())).ok()?;
    fdinfo.lines().find_map(|line| line.strip_prefix("Pid:"))?.trim().parse().ok().filter(|pid| *pid > 0)
}

fn alive(pidfd: &OwnedFd) -> bool {
    let mut exited = [rustix::event::PollFd::new(pidfd, rustix::event::PollFlags::IN)];
    rustix::event::poll(&mut exited, 0).is_ok_and(|ready| ready == 0)
}

fn session_of(process: &Path) -> Option<String> {
    let environ = std::fs::read(process.join("environ")).ok()?;
    environ.split(|byte| *byte == 0).find_map(|entry| entry.strip_prefix(b"NORTH_FLEET_SESSION=")).filter(|session| !session.is_empty()).map(|session| String::from_utf8_lossy(session).into_owned())
}

fn unit_of(process: &Path) -> Option<String> {
    let cgroup = std::fs::read_to_string(process.join("cgroup")).ok()?;
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    path.rsplit('/').find(|part| part.ends_with(".service") || part.ends_with(".scope")).map(str::to_owned)
}

fn keep_quota(directory: &Path, request: &Request, response: &Response) {
    let number = |name: &str| response.header(name).and_then(|value| value.trim().parse::<u64>().ok());
    let (Some(remaining), Some(resets_at), Some(resource)) = (number("x-ratelimit-remaining"), number("x-ratelimit-reset"), response.header("x-ratelimit-resource")) else {
        return;
    };
    crate::platform::github_quota::record(directory, request.header("authorization").unwrap_or(""), resource, remaining, resets_at);
}

pub fn token_kind(request: &Request) -> &'static str {
    let credential = request.header("authorization").and_then(|value| value.split_whitespace().nth(1)).unwrap_or("");
    match credential {
        "" => "none",
        token if token.starts_with("ghs_") => "app",
        _ => "personal",
    }
}
