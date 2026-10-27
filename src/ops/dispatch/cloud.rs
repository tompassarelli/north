//! The cloud credit lane's launch: a tiny checkout of the repository under /tmp/cloud/src, so the
//! cloud session may push to it, the trust entry the login needs for that folder, and
//! `claude --cloud` started from it.
use super::issues::{IssueRef, OWNER};
use super::record::{self, Record};
use crate::platform::process::status_within;
use crate::platform::terminal;
use regex::Regex;
use serde_json::{Value, json};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

pub(crate) const SOURCES: &str = "/tmp/cloud/src";
pub(crate) const AUTOLAND: [&str; 2] = ["smashcraft", "wisp"];
pub(crate) const HOST: &str = "anthropic-cloud";
pub(crate) const SESSIONS: i64 = 6;
const LAUNCH_WAIT: Duration = Duration::from_secs(180);
static SESSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"session_[A-Za-z0-9]+").expect("session pattern"));

pub(crate) type Run<'a> = dyn FnMut(&Path, &str, &[String]) -> Result<String, String> + 'a;

#[derive(Debug, PartialEq)]
pub(crate) struct Launched {
    pub session: String,
    pub branch: String,
    pub directory: PathBuf,
}

pub(crate) fn branch(slug: &str) -> String {
    format!("claude/land-{slug}")
}

pub(crate) fn autoland(repo: &str) -> bool {
    AUTOLAND.contains(&repo)
}

pub(crate) fn checkout_steps(repo: &str, directory: &Path, fresh: bool) -> Vec<Vec<String>> {
    let dir = directory.to_string_lossy();
    let url = format!("https://github.com/{OWNER}/{repo}.git");
    let mut steps: Vec<Vec<&str>> = Vec::new();
    if fresh {
        steps.push(vec!["init", "-q", &dir]);
    }
    steps.extend([
        vec!["-C", &dir, "config", "remote.origin.url", &url],
        vec!["-C", &dir, "config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"],
        vec!["-C", &dir, "fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", "main"],
        vec!["-C", &dir, "sparse-checkout", "set", "--no-cone", "/README.md"],
        vec!["-C", &dir, "checkout", "-q", "-B", "main", "FETCH_HEAD"],
        vec!["-C", &dir, "branch", "-q", "--set-upstream-to=origin/main", "main"],
    ]);
    steps.into_iter().map(|step| step.into_iter().map(str::to_owned).collect()).collect()
}

/// Running sessions rewrite `.claude.json` and drop this entry, so it is written just before each launch.
pub(crate) fn trust(config: &Path, directory: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(config).map_err(|error| format!("{}: {error}", config.display()))?;
    let mut value: Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", config.display()))?;
    let root = value.as_object_mut().ok_or_else(|| format!("{} is not a JSON object", config.display()))?;
    let projects = root.entry("projects").or_insert_with(|| json!({}));
    let projects = projects.as_object_mut().ok_or_else(|| format!("{} projects is not an object", config.display()))?;
    let project = projects.entry(directory.to_string_lossy().into_owned()).or_insert_with(|| json!({}));
    let project = project.as_object_mut().ok_or_else(|| format!("{} project entry is not an object", config.display()))?;
    project.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    let temporary = config.with_extension(format!("json.north-{}", std::process::id()));
    write_private(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, config)).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("{}: {error}", config.display())
    })
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub(crate) fn launch(repo: &str, slug: &str, brief: &str, model: &[String], sources: &Path, home: &Path, run: &mut Run) -> Result<Launched, String> {
    let directory = sources.join(repo);
    let fresh = !directory.join(".git").exists();
    for step in checkout_steps(repo, &directory, fresh) {
        run(Path::new("/"), "git", &step).map_err(|why| format!("git {}: {why}", step.join(" ")))?;
    }
    trust(&home.join(".claude.json"), &directory)?;
    let arguments: Vec<String> = model.iter().cloned().chain(["--cloud".to_owned(), brief.to_owned()]).collect();
    let printed = run(&directory, "claude", &arguments).map_err(|why| format!("claude --cloud: {why}"))?;
    let session = SESSION.find(&printed).map(|found| found.as_str().to_owned()).ok_or_else(|| format!("claude --cloud printed no session_ ID: {}", printed.trim()))?;
    Ok(Launched { session, branch: branch(slug), directory })
}

pub(crate) fn item(text: &str) -> Result<IssueRef, String> {
    IssueRef::parse(text).filter(|item| item.to_string() == text.to_lowercase()).ok_or_else(|| format!("--item {text} is not REPO#N"))
}

pub(crate) fn hold(item: &IssueRef, launched: &Launched, here: &str, pace: &Path, at: f64) -> Result<(), String> {
    let detail = format!("north cloud launch {} {}", launched.session, launched.branch);
    let entry = Record { at, item: item.to_string(), host: HOST.into(), account: format!("claude login on {here}"), name: launched.session.clone(), cloud: true, launched: true, detail, tier: None, category: None, arm: false };
    record::append(pace, &entry)
}

pub(crate) fn run(directory: &Path, program: &str, args: &[String]) -> Result<String, String> {
    run_from(directory, program, &crate::fleet::runtime::binary(program), args)
}

pub(crate) fn run_from(directory: &Path, program: &str, path: &Path, args: &[String]) -> Result<String, String> {
    let mut command = Command::new(path);
    command.args(args).current_dir(directory);
    let captured = if program == "claude" { terminal::capture(command, LAUNCH_WAIT) } else { in_file(command) };
    match captured.map_err(|error| error.to_string())? {
        (None, _) => Err(format!("did not return within {} s", LAUNCH_WAIT.as_secs())),
        (Some(status), text) if status.success() => Ok(text),
        (Some(status), text) => Err(format!("{status}: {}", text.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default())),
    }
}

fn in_file(mut command: Command) -> std::io::Result<(Option<ExitStatus>, String)> {
    let mut output = tempfile::tempfile()?;
    command.stdin(Stdio::null()).stdout(output.try_clone()?).stderr(output.try_clone()?);
    let status = status_within(&mut command, LAUNCH_WAIT)?;
    let mut text = String::new();
    output.rewind().and_then(|()| output.read_to_string(&mut text))?;
    Ok((status, text))
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
