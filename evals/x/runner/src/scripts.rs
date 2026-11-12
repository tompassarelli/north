use std::env;
use std::fs;
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
    let t_arm = evals.join("t/arm");
    let refuse = |why: String| {
        Err(RunError::Refused(format!(
            "north runtime {} {why}; the run stops before its first trial",
            runtime.unwrap_or("(none)")
        )))
    };
    if let Some(path) = X_SCRIPTS
        .iter()
        .map(|script| eval_x.join(script))
        .chain([t_arm.clone()])
        .find(|path| !path.is_file())
    {
        return refuse(format!("has no {}", path.display()));
    }
    let arm = fs::read_to_string(&t_arm)?;
    let calls_cargo = arm
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word == "cargo");
    if calls_cargo || !arm.contains("EVAL_T_TOOL") {
        return refuse(format!(
            "has a {} that predates this change and builds its own tool",
            t_arm.display()
        ));
    }
    match env::var_os("EVAL_T_TOOL") {
        Some(tool) if Path::new(&tool).is_file() => Ok(()),
        _ => Err(RunError::Refused(
            "EVAL_T_TOOL does not name a built eval-t binary; evals/x/run builds it and exports it"
                .to_owned(),
        )),
    }
}
