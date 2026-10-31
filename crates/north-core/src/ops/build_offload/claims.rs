use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One build this process sends to a host, kept as a file so builds asking at the same moment count each other before the host's load shows them.
pub(super) struct Claim {
    path: PathBuf,
    host: String,
    stamp: String,
}

fn directory() -> PathBuf {
    crate::capacity::agent_load::capacity_root().join("offloads")
}

fn name(host: &str, stamp: &str, pid: u32) -> String {
    format!("{host}.{stamp}.{pid}")
}

fn live(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn parts(file: &str) -> Option<(&str, &str, u32)> {
    let (rest, pid) = file.rsplit_once('.')?;
    let (host, stamp) = rest.rsplit_once('.')?;
    Some((host, stamp, pid.parse().ok()?))
}

impl Claim {
    pub(super) fn take(host: &str) -> Option<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
        let stamp = format!("{nanos:040}");
        let directory = directory();
        std::fs::create_dir_all(&directory).ok()?;
        let path = directory.join(name(host, &stamp, std::process::id()));
        std::fs::write(&path, b"").ok()?;
        Some(Self { path, host: host.to_owned(), stamp })
    }

    /// The live builds that claimed this host before this one.
    pub(super) fn ahead(&self) -> usize {
        let Ok(entries) = std::fs::read_dir(directory()) else { return 0 };
        entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|file| parts(file).is_some_and(|(host, stamp, pid)| host == self.host && stamp < self.stamp.as_str() && live(pid)))
            .count()
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_a_claim_counts_only_live_earlier_claims_on_its_own_host() {
        assert_eq!(parts("gex45.00000000000000000000000000000000000012.4242"), Some(("gex45", "00000000000000000000000000000000000012", 4242)));
        let first = Claim::take("claims-test-host").unwrap();
        let second = Claim::take("claims-test-host").unwrap();
        let other = Claim::take("claims-test-other").unwrap();
        assert_eq!((first.ahead(), second.ahead(), other.ahead()), (0, 1, 0));
        drop(first);
        assert_eq!(second.ahead(), 0);
    }
}
