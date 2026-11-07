use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const NAMED_LOG: &str = "NORTH_PUSH_LOG";

pub struct Push<'a> {
    pub at_secs: u64,
    pub repository: &'a str,
    pub branch: &'a str,
    pub oid: &'a str,
    pub landed: &'a str,
    pub session: &'a str,
}

pub fn repository(origin: &str) -> String {
    let name = origin.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(origin);
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

pub fn log(state_home: &Path) -> PathBuf {
    named().unwrap_or_else(|| host_log(state_home))
}

pub fn landing_log(repo: &Path, state_home: &Path, code_root: &Path) -> Option<PathBuf> {
    named().or_else(|| is_host_checkout(repo, code_root).then(|| host_log(state_home)))
}

fn named() -> Option<PathBuf> {
    std::env::var_os(NAMED_LOG).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn host_log(state_home: &Path) -> PathBuf {
    state_home.join("safe-push/pushes.tsv")
}

fn is_host_checkout(repo: &Path, code_root: &Path) -> bool {
    let resolved = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved(repo).starts_with(resolved(code_root))
}

pub fn record(path: &Path, push: &Push) -> std::io::Result<bool> {
    let known = fs::read_to_string(path).unwrap_or_default().lines().any(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        fields.get(1) == Some(&push.repository)
            && fields.get(2) == Some(&push.branch)
            && fields.get(3) == Some(&push.oid)
            && fields.get(4) == Some(&push.landed)
    });
    if known {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = format!("{}\t{}\t{}\t{}\t{}\t{}\n", push.at_secs, push.repository, push.branch, push.oid, push.landed, push.session);
    OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes()).map(|()| true)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_host_checkout;

    #[test]
    fn a_lander_serving_a_scratch_repository_outside_the_code_root_records_no_landing() {
        let code = Path::new("/nonexistent-home/code");
        assert!(!is_host_checkout(Path::new("/tmp/vn-exp.tZfk/proj/main"), code), "n719-verify's scratch lander of 2026-11-08 recorded landed a9c5cb53c7");
        assert!(is_host_checkout(Path::new("/nonexistent-home/code/north/main"), code));
    }
}
