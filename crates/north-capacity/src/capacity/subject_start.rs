//! The witness that a lease's subject started. `north capacity host run` names a file in `NORTH_SUBJECT_STARTED`, and the
//! innermost wrapper (`north capacity exec`, or the dev shell's direct run) creates it just before it becomes the subject.
//! A lease that exits nonzero without the file never ran its subject: systemd-run, the deadline or the dev shell failed
//! first, and that failure is the host's, never the subject's verdict.
use std::path::{Path, PathBuf};

use north_base::foundation::exit::NO_VERDICT_STATUS;

pub const ENV: &str = "NORTH_SUBJECT_STARTED";

/// Where the lease's witness lives under the capacity root.
pub fn witness(root: &Path, lease: &str) -> PathBuf {
    root.join("started").join(lease)
}

/// Clears the lease's witness and its start's stamps before its launch, so a stale file never vouches for a subject that did not start.
pub fn arm(witness: &Path) {
    let _ = std::fs::remove_file(witness);
    let _ = std::fs::remove_file(stamps(witness));
    if let Some(parent) = witness.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

fn stamps(witness: &Path) -> PathBuf {
    witness.with_extension("steps")
}

fn epoch(at: std::time::SystemTime) -> f64 {
    at.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
}

/// The most of the start's stamps a lease reads back, since the subject can write the file they sit in.
const STAMPS_BOUND: u64 = 64 * 1024;

/// The name of a lease's first start step, from its launch to the first stamp: the scope and the wrapper process starting.
pub const LAUNCH_STEP: &str = "scope";

/// Records that the lease's start begins the step `name` now, in the stamps beside the witness this process inherited, so the lease can say what its start spent before the subject ran.
pub(crate) fn step(name: &str) {
    use std::io::Write;
    let Some(witness) = std::env::var_os(ENV).filter(|value| !value.is_empty()).map(PathBuf::from) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(stamps(&witness)) {
        let _ = writeln!(file, "{name} {:.3}", epoch(std::time::SystemTime::now()));
    }
}

/// What a lease spent between its launch and its subject's start, step by step, and how long the subject then ran until the lease's command exited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Start {
    pub steps: Vec<(String, f64)>,
    pub subject_seconds: Option<f64>,
}

/// The lease's start as the stamps beside `witness` record it, from `launched` to the subject's start (the witness's time), or to `ended` when the subject never started; reading the stamps removes them.
pub fn start(witness: &Path, launched: f64, ended: f64) -> Start {
    use std::io::Read;
    let stamps_path = stamps(witness);
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(&stamps_path) {
        let _ = file.take(STAMPS_BOUND).read_to_string(&mut text);
    }
    let _ = std::fs::remove_file(&stamps_path);
    let subject = std::fs::metadata(witness).and_then(|metadata| metadata.modified()).ok().map(epoch).filter(|subject| subject.is_finite());
    let until = subject.unwrap_or(ended).max(launched);
    let mut steps = Vec::new();
    let (mut name, mut at) = (LAUNCH_STEP.to_owned(), launched);
    for (next, stamp) in text.lines().filter_map(|line| line.rsplit_once(' ')).filter_map(|(next, stamp)| Some((next, stamp.parse::<f64>().ok().filter(|stamp| stamp.is_finite())?))) {
        let stamp = stamp.max(at).min(until);
        steps.push((std::mem::replace(&mut name, next.to_owned()), stamp - at));
        at = stamp;
    }
    steps.push((name, until - at));
    Start { steps, subject_seconds: subject.map(|subject| (ended - subject).max(0.0)) }
}

/// Records that the subject is starting, in the witness this process inherited; returns it so a failed exec can take it back.
pub(crate) fn mark() -> Option<PathBuf> {
    let witness = std::env::var_os(ENV).filter(|value| !value.is_empty()).map(PathBuf::from)?;
    std::fs::write(&witness, b"").ok().map(|()| witness)
}

pub(crate) fn unmark(witness: Option<PathBuf>) {
    if let Some(witness) = witness {
        let _ = std::fs::remove_file(witness);
    }
}

/// The lease's exit as the subject's verdict: a nonzero exit with no witness is no verdict, except the innermost wrapper's own 127 or 126, its verdict that the named program is missing or not executable, which is the command's and keeps its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Subject(i32),
    NotStarted { launcher: i32 },
}

impl Exit {
    pub fn judge(exit_code: i32, witness: &Path) -> Self {
        let started = witness.exists();
        let _ = std::fs::remove_file(witness);
        let unrunnable = [super::dev_shell::NOT_FOUND, super::dev_shell::NOT_EXECUTABLE].map(i32::from).contains(&exit_code);
        if exit_code == 0 || started || unrunnable { Self::Subject(exit_code) } else { Self::NotStarted { launcher: exit_code } }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::Subject(code) => code,
            Self::NotStarted { .. } => NO_VERDICT_STATUS,
        }
    }
}
