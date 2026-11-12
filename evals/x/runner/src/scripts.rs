use std::path::Path;

use crate::error::{Result, RunError};

const X_SCRIPTS: [&str; 6] = [
    "arm",
    "host",
    "handoff/args",
    "workspace/seed",
    "relay/broker",
    "relay/container-bash",
];

pub fn require(eval_x: &Path, runtime: Option<&str>) -> Result<()> {
    let evals = eval_x.parent().unwrap_or(eval_x);
    let missing = X_SCRIPTS
        .iter()
        .map(|script| eval_x.join(script))
        .chain([evals.join("t/arm")])
        .find(|path| !path.is_file());
    match missing {
        None => Ok(()),
        Some(path) => Err(RunError::Refused(format!(
            "north runtime {} has no {}; the run stops before its first trial",
            runtime.unwrap_or("(none)"),
            path.display()
        ))),
    }
}
