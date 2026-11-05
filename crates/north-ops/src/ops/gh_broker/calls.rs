use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::broker::Source;
use super::http::{Request, Response};

pub struct CallLog {
    path: Option<PathBuf>,
}

impl CallLog {
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    #[cfg(test)]
    pub fn off() -> Self {
        Self { path: None }
    }

    pub fn record(&self, stream: &UnixStream, request: &Request, response: &Response, source: Source) {
        let Some(path) = &self.path else {
            return;
        };
        let caller = caller(stream);
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let line = format!("{at}\t{caller}\t{}\t{}\t{}\t{}\n", source.name(), response.status, request.method, request.target);
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(line.as_bytes()));
    }
}

fn caller(stream: &UnixStream) -> String {
    let Ok(peer) = rustix::net::sockopt::get_socket_peercred(stream) else {
        return "unknown".to_owned();
    };
    let pid = peer.pid.as_raw_nonzero().get();
    session_of(Path::new(&format!("/proc/{pid}"))).unwrap_or_else(|| format!("pid-{pid}"))
}

fn session_of(process: &Path) -> Option<String> {
    let environ = std::fs::read(process.join("environ")).ok()?;
    environ.split(|byte| *byte == 0).find_map(|entry| entry.strip_prefix(b"NORTH_FLEET_SESSION=")).filter(|session| !session.is_empty()).map(|session| String::from_utf8_lossy(session).into_owned())
}
