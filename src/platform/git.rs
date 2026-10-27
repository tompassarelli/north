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

pub(crate) fn run_without_local_env(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    let listing = process::run("git", None, &["rev-parse", "--local-env-vars"])?;
    if !listing.status.success() {
        return Err(std::io::Error::other("git rev-parse --local-env-vars failed"));
    }
    let mut command = std::process::Command::new("git");
    command.current_dir(dir).args(args);
    for name in String::from_utf8_lossy(&listing.stdout).lines().filter(|name| !name.is_empty()) {
        let valid = name.starts_with("GIT_")
            && name.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if !valid {
            return Err(std::io::Error::other(format!("invalid Git local environment name: {name}")));
        }
        command.env_remove(name);
    }
    command.output()
}
