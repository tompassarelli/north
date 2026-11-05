use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn repository(origin: &str) -> String {
    let name = origin.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(origin);
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

pub fn epoch() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

pub fn seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn directory(state_home: &Path) -> PathBuf {
    state_home.join("safe-push")
}

fn append(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}

pub struct Push<'a> {
    pub at_secs: u64,
    pub repository: &'a str,
    pub branch: &'a str,
    pub oid: &'a str,
    pub landed: &'a str,
    pub session: &'a str,
}

pub fn record_push(state_home: &Path, push: &Push) -> std::io::Result<bool> {
    let path = directory(state_home).join("pushes.tsv");
    let known = fs::read_to_string(&path).unwrap_or_default().lines().any(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        fields.get(1) == Some(&push.repository) && fields.get(2) == Some(&push.branch) && fields.get(3) == Some(&push.oid) && fields.get(4) == Some(&push.landed)
    });
    if known {
        return Ok(false);
    }
    append(&path, &format!("{}\t{}\t{}\t{}\t{}\t{}", push.at_secs, push.repository, push.branch, push.oid, push.landed, push.session))?;
    Ok(true)
}

pub struct Stages {
    pub attempts: usize,
    pub bisect_depth: usize,
    started_secs: u64,
    started_epoch: f64,
    started: Instant,
    last: Instant,
    marks: Vec<(String, u128, u128)>,
    checked: Option<String>,
    pushing: Option<f64>,
}

impl Stages {
    pub fn start() -> Self {
        let now = Instant::now();
        Self { attempts: 1, bisect_depth: 0, started_secs: seconds(), started_epoch: epoch(), started: now, last: now, marks: Vec::new(), checked: None, pushing: None }
    }

    pub fn empty() -> Self {
        let mut stages = Self::start();
        stages.marks.clear();
        stages
    }

    pub fn mark(&mut self, stage: &str) {
        let now = Instant::now();
        self.marks.push((stage.to_owned(), now.duration_since(self.last).as_millis(), now.duration_since(self.started).as_millis()));
        self.last = now;
    }

    pub fn checked(&mut self, head: &str) {
        self.mark("check");
        self.checked = Some(head.to_owned());
    }

    pub fn pushing(&mut self) {
        self.pushing = Some(epoch());
    }

    pub fn checked_head(&self) -> Option<&str> {
        self.checked.as_deref()
    }

    pub fn pushing_at(&self) -> Option<f64> {
        self.pushing
    }

    pub fn started_at(&self) -> f64 {
        self.started_epoch
    }

    pub fn check_window(&self) -> Option<(f64, f64)> {
        self.window("check")
    }

    pub fn window(&self, name: &str) -> Option<(f64, f64)> {
        let (_, stage_ms, total_ms) = self.marks.iter().find(|(stage, _, _)| stage == name)?;
        Some((self.started_epoch + (total_ms - stage_ms) as f64 / 1000.0, self.started_epoch + *total_ms as f64 / 1000.0))
    }

    pub fn flush(&self, state_home: &Path, repository: &str, branch: &str) -> std::io::Result<()> {
        let path = directory(state_home).join("stages.tsv");
        for (stage, stage_ms, total_ms) in &self.marks {
            append(&path, &format!("{}\t{repository}\t{branch}\t{stage}\t{stage_ms}\t{total_ms}", self.started_secs))?;
        }
        Ok(())
    }
}
