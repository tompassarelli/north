use super::process;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

pub(crate) fn run(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    process::run("git", Some(dir), args)
}

pub(crate) fn stdout(
    dir: &Path,
    args: &[&str],
    describe_failure: impl FnOnce(&Output) -> String,
) -> Result<Vec<u8>, String> {
    let output = run(dir, args).map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(describe_failure(&output));
    }
    Ok(output.stdout)
}

pub(crate) fn run_within(args: &[&str], limit: Duration) -> Option<Output> {
    process::run_within("git", args, limit)
}
