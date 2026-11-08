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

/// Clears the lease's witness before its launch, so a stale file never vouches for a subject that did not start.
pub fn arm(witness: &Path) {
    let _ = std::fs::remove_file(witness);
    if let Some(parent) = witness.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
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
