use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::broker::Source;
use crate::platform::{session_scope, socket_peer};
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

/// Who called the broker, read through the peer's pidfd: `name` for the call log (the fleet session its environment names, else its unit, else its pid), `cgroup`, which the caller cannot change, and `ours`, whether it runs in the broker's own user namespace. The caller sets its own environment, so only `cgroup` decides what the broker adds or lends.
pub struct Caller {
    pub name: String,
    pub cgroup: Option<String>,
    pub ours: bool,
}

impl Caller {
    pub fn in_worker_scope(&self) -> bool {
        self.cgroup.as_deref().is_some_and(|cgroup| session_scope::worker_cgroup(cgroup) && !session_scope::lane_only_cgroup(cgroup))
    }

    /// The service or scope its cgroup names, for a caller in the broker's own user namespace only.
    pub fn unit(&self) -> Option<&str> {
        self.cgroup.as_deref().filter(|_| self.ours).and_then(leaf_unit)
    }
}

pub fn caller(stream: &UnixStream) -> Caller {
    let held = socket_peer::hold(stream, |pid| {
        let process = PathBuf::from(format!("/proc/{pid}"));
        (session_of(&process), std::fs::read_to_string(process.join("cgroup")).ok(), socket_peer::shares_user_namespace(pid))
    });
    let Some(held) = held else {
        return Caller { name: "unknown".to_owned(), cgroup: None, ours: false };
    };
    let (session, cgroup, ours) = held.read;
    let name = session.or_else(|| cgroup.as_deref().and_then(leaf_unit).map(str::to_owned)).unwrap_or_else(|| format!("pid-{}", held.pid));
    Caller { name, cgroup, ours }
}

fn session_of(process: &Path) -> Option<String> {
    let environ = std::fs::read(process.join("environ")).ok()?;
    environ.split(|byte| *byte == 0).find_map(|entry| entry.strip_prefix(b"NORTH_FLEET_SESSION=")).filter(|session| !session.is_empty()).map(|session| String::from_utf8_lossy(session).into_owned())
}

fn leaf_unit(cgroup: &str) -> Option<&str> {
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    path.rsplit('/').find(|part| part.ends_with(".service") || part.ends_with(".scope"))
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
