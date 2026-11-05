use super::failure::Failure;
use super::layout::Layout;
use super::{deploy, update};
use crate::platform::process;
use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn follow(layout: &Layout, repo: &Path) -> Result<(), Failure> {
    if !names_north(&origin(repo)) {
        return Ok(());
    }
    let log = OpenOptions::new().create(true).append(true).open(layout.state.join("runtime-follow.log"))?;
    Command::new(std::env::current_exe()?)
        .args(["runtime", "promote"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let here = update::update(layout);
    let elsewhere = deploy::deploy(layout, &[]);
    here.and(elsewhere)
}

fn origin(repo: &Path) -> String {
    process::run("git", Some(repo), &["remote", "get-url", "origin"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn names_north(url: &str) -> bool {
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    url.rsplit(['/', ':']).next() == Some("north")
}

