use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::store::{Pass, Store, hex};

mod sync;

pub use sync::{Replica, Report, exchange, sync};

pub(super) const ROOT: &str = "v2";

fn lowercase_hex(text: &str) -> bool {
    hex(text) && !text.bytes().any(|byte| byte.is_ascii_uppercase())
}

fn require_hex(what: &str, text: &str) -> io::Result<()> {
    if lowercase_hex(text) { Ok(()) } else { Err(io::Error::other(format!("verdict cache {what} {text:?} is not lowercase hex"))) }
}

fn put(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    if directory.join(name).exists() {
        return Ok(());
    }
    fs::create_dir_all(directory)?;
    let pending = tempfile::NamedTempFile::new_in(directory)?;
    fs::write(pending.path(), bytes)?;
    match pending.persist_noclobber(directory.join(name)) {
        Err(error) if error.error.kind() != io::ErrorKind::AlreadyExists => Err(error.error),
        _ => Ok(()),
    }
}

struct Strong {
    passes: PathBuf,
    cleared: PathBuf,
    name: String,
}

impl Store {
    fn directory(&self, weak: &str, kind: &str) -> io::Result<PathBuf> {
        require_hex("weak key", weak)?;
        Ok(self.0.join(ROOT).join(weak).join(kind))
    }

    fn strong(&self, weak: &str, strong: &str) -> io::Result<Strong> {
        require_hex("strong key", strong)?;
        Ok(Strong { passes: self.directory(weak, "pass")?, cleared: self.directory(weak, "cleared")?, name: strong.to_owned() })
    }

    pub fn record_set(&self, weak: &str, bytes: &[u8]) -> io::Result<String> {
        let id: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        put(&self.directory(weak, "sets")?, &id, bytes)?;
        Ok(id)
    }

    pub fn sets(&self, weak: &str) -> io::Result<Vec<Vec<u8>>> {
        let directory = self.directory(weak, "sets")?;
        let Ok(entries) = fs::read_dir(&directory) else { return Ok(Vec::new()) };
        let mut names: Vec<String> = entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok()).filter(|name| lowercase_hex(name)).collect();
        names.sort();
        Ok(names.iter().filter_map(|name| fs::read(directory.join(name)).ok()).collect())
    }

    pub fn record_pass(&self, weak: &str, strong: &str, pass: &Pass, host: &str) -> io::Result<()> {
        let at = self.strong(weak, strong)?;
        if host.is_empty() || host.bytes().any(|byte| byte.is_ascii_whitespace()) || !lowercase_hex(&pass.sha) {
            return Err(io::Error::other(format!("verdict cache pass for host {host:?} and sha {:?} is not recordable", pass.sha)));
        }
        if at.cleared.join(&at.name).exists() {
            return Ok(());
        }
        put(&at.passes, &at.name, format!("{} {} {host}\n", pass.at.unix_seconds(), pass.sha).as_bytes())
    }

    pub fn pass(&self, weak: &str, strong: &str) -> io::Result<Option<(Pass, String)>> {
        let at = self.strong(weak, strong)?;
        if at.cleared.join(&at.name).exists() {
            return Ok(None);
        }
        let Ok(text) = fs::read_to_string(at.passes.join(&at.name)) else { return Ok(None) };
        let mut words = text.trim().splitn(3, ' ');
        let (Some(seconds), Some(sha), Some(host)) = (words.next(), words.next(), words.next()) else { return Ok(None) };
        Ok(Pass::parse(&format!("{seconds} {sha}")).map(|pass| (pass, host.to_owned())))
    }

    pub fn clear(&self, weak: &str, strong: &str) -> io::Result<()> {
        let at = self.strong(weak, strong)?;
        put(&at.cleared, &at.name, b"")?;
        match fs::remove_file(at.passes.join(&at.name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}
