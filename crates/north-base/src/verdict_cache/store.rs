use std::fs;
use std::io;
use std::path::PathBuf;

use crate::foundation::clock::Timestamp;

pub const DIR_ENV: &str = "NORTH_VERDICT_CACHE_DIR";
const REMOTE_DIR: &str = "\"${XDG_STATE_HOME:-$HOME/.local/state}/north/verdict-cache\"";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pass {
    pub at: Timestamp,
    pub sha: String,
}

impl Pass {
    pub fn parse(text: &str) -> Option<Self> {
        let (seconds, sha) = text.trim().split_once(' ')?;
        hex(sha).then(|| Some(Self { at: Timestamp::from_unix_seconds(seconds.parse().ok()?), sha: sha.to_owned() })).flatten()
    }

    fn line(&self) -> String {
        format!("{} {}\n", self.at.unix_seconds(), self.sha)
    }
}

fn hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub struct Store(pub PathBuf);

impl Store {
    pub fn here() -> Self {
        if let Some(dir) = std::env::var_os(DIR_ENV) {
            return Self(dir.into());
        }
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self(state.join("north/verdict-cache"))
    }

    pub fn hit(&self, key: &str) -> Option<Pass> {
        hex(key).then(|| Pass::parse(&fs::read_to_string(self.0.join(key)).ok()?)).flatten()
    }

    pub fn record(&self, key: &str, pass: Option<&Pass>) -> io::Result<()> {
        if !hex(key) {
            return Err(io::Error::other(format!("verdict cache key {key:?} is not hex")));
        }
        let path = self.0.join(key);
        let Some(pass) = pass else {
            return match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        };
        fs::create_dir_all(&self.0)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0)?;
        fs::write(pending.path(), pass.line())?;
        pending.persist(&path).map(|_| ()).map_err(|error| error.error)
    }
}

pub fn remote_hits(keys: &[String]) -> String {
    let keys: Vec<&str> = keys.iter().map(String::as_str).filter(|key| hex(key)).collect();
    format!("d={REMOTE_DIR}; for k in {}; do [ -f \"$d/$k\" ] && printf '%s %s\\n' \"$k\" \"$(cat \"$d/$k\")\"; done; exit 0", keys.join(" "))
}

pub fn remote_record(key: &str, pass: Option<&Pass>) -> Option<String> {
    if !hex(key) || pass.is_some_and(|pass| !hex(&pass.sha)) {
        return None;
    }
    Some(match pass {
        Some(pass) => format!("d={REMOTE_DIR}; mkdir -p \"$d\"; t=$(mktemp \"$d/.pending.XXXXXX\"); printf '%s' '{}' >\"$t\"; mv -f \"$t\" \"$d/{key}\"", pass.line()),
        None => format!("rm -f {REMOTE_DIR}/{key}"),
    })
}

pub fn reused_line(command: &str, key: &str, host: &str, pass: &Pass) -> String {
    format!("land check: {command} reused from the verdict cache on {host} (reused verdict {}, passed for these inputs at {}, {})", key.get(..12).unwrap_or(key), pass.at.local_twelve_hour(), pass.sha.get(..12).unwrap_or(&pass.sha))
}
