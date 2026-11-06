use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

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

pub fn record(state_home: &Path, push: &Push) -> std::io::Result<bool> {
    let path = state_home.join("safe-push/pushes.tsv");
    let known = fs::read_to_string(&path).unwrap_or_default().lines().any(|line| {
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
    OpenOptions::new().create(true).append(true).open(&path)?.write_all(line.as_bytes()).map(|()| true)
}
