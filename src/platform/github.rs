use super::process;
use std::process::Output;

pub(crate) fn run(args: &[&str]) -> std::io::Result<Output> {
    process::run("gh", None, args)
}

pub(crate) fn run_owned(args: &[String]) -> std::io::Result<Output> {
    process::run("gh", None, args)
}

pub(crate) async fn run_async(args: &[String]) -> std::io::Result<Output> {
    process::run_killing_on_drop("gh", args).await
}

pub(crate) fn json(args: &[&str]) -> Result<serde_json::Value, String> {
    let out = run(args).map_err(|error| error.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice(&out.stdout).map_err(|error| error.to_string())
}
