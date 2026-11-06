use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::broker::Source;
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

    pub fn record(&self, stream: &UnixStream, request: &Request, response: &Response, source: Source) {
        if let Some(directory) = &self.quota {
            keep_quota(directory, request, response);
        }
        let Some(path) = &self.path else {
            return;
        };
        let caller = caller(stream);
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let line = format!("{at}\t{caller}\t{}\t{}\t{}\t{}\n", source.name(), response.status, request.method, request.target);
        if std::fs::metadata(path).is_ok_and(|meta| meta.len() > ROTATE_BYTES) {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

fn caller(stream: &UnixStream) -> String {
    let Ok(peer) = rustix::net::sockopt::get_socket_peercred(stream) else {
        return "unknown".to_owned();
    };
    let pid = peer.pid.as_raw_nonzero().get();
    let process = PathBuf::from(format!("/proc/{pid}"));
    session_of(&process).or_else(|| unit_of(&process)).unwrap_or_else(|| format!("pid-{pid}"))
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
