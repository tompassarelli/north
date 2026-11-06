use super::super::bounded;
use super::super::failure::{Failure, code, failed};
use super::super::layout::{self, Layout};
use super::super::remote;
use north_base::agents_sync::POLICY_HELD;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const STEP_LIMIT: Duration = Duration::from_secs(300);
const LOG_TAIL: usize = 8;

pub(super) fn swap(layout: &Layout, link: &Path) -> std::io::Result<()> {
    let staged = layout.state.join("current.tmp");
    let _ = fs::remove_file(&staged);
    symlink(link, &staged)?;
    fs::rename(&staged, layout.current())
}

pub(super) fn sync(layout: &Layout, previous: Option<&Path>, target: &str) -> Result<String, Failure> {
    let steps = vec![("health check", health_check(layout)), ("agents sync", agents_sync(&layout.runtime(target).join("north")))];
    settle(layout, previous, target, steps, &mut |text: &str| alert_proxy(&format!("north runtime update: {text}")))
}

pub(super) fn settle(layout: &Layout, previous: Option<&Path>, target: &str, steps: Vec<(&str, Command)>, notify: &mut dyn FnMut(&str)) -> Result<String, Failure> {
    let log = tempfile::Builder::new()
        .prefix(&format!("runtime-sync-{}.", &target[..8]))
        .suffix(".log")
        .tempfile_in(&layout.state)?;
    swap(layout, &layout::link(target))?;
    let mut failure = None;
    let mut held = false;
    let limit = remote::limit(STEP_LIMIT);
    let append = || fs::OpenOptions::new().append(true).open(log.path());
    let note = |line: String| {
        let _ = append().and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
    };
    for (name, mut command) in steps {
        let mut attempt = || bounded::status(command.stdout(append()?).stderr(append()?), limit);
        let ran = match attempt() {
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                note(format!("{name} timed out after {}s; retrying once\n", limit.as_secs()));
                attempt()
            }
            ran => ran,
        };
        match ran {
            Ok(status) if status.success() => continue,
            Ok(status) if name == "agents sync" && code(status) == POLICY_HELD => {
                held = true;
                continue;
            }
            Ok(status) => failure = Some((name, code(status))),
            Err(error) => {
                note(format!("{name} did not run: {error}\n"));
                failure = Some((name, 127));
            }
        }
        break;
    }
    let Some((step, status)) = failure else {
        let mut synced = fs::read_to_string(log.path()).unwrap_or_default();
        policy_notice(layout, target, held.then_some(synced.as_str()), notify);
        if !synced.is_empty() && !synced.ends_with('\n') {
            synced.push('\n');
        }
        return Ok(synced);
    };
    if let Some(previous) = previous {
        swap(layout, previous)?;
    }
    let logged = fs::read_to_string(log.path()).unwrap_or_default();
    for line in log_tail(&logged) {
        eprintln!("north runtime update: {step} log: {line}");
    }
    let (_, kept) = log.keep().map_err(|error| failed(1, error.to_string()))?;
    let was = previous.map_or_else(|| "<none>".to_owned(), |link| link.display().to_string());
    let message = format!(
        "failed step {step} on {target}; rolled back to {was}; exit status {status}; local log: {}",
        kept.display()
    );
    notify(&message);
    Err(failed(1, message))
}

fn policy_notice(layout: &Layout, target: &str, held: Option<&str>, notify: &mut dyn FnMut(&str)) {
    let record = layout.state.join("policy-held");
    let Some(logged) = held else {
        let _ = fs::remove_file(&record);
        return;
    };
    let cause = logged
        .lines()
        .find(|line| line.starts_with("agents sync: ") && !line.contains("generation not published"))
        .unwrap_or("agents sync: the policy gate refused the generation");
    if fs::read_to_string(&record).is_ok_and(|seen| seen == cause) {
        return;
    }
    let _ = fs::write(&record, cause);
    notify(&format!("runtime {} is live; the policy generation is held and the last published one stays live: {cause}", &target[..8]));
}

fn log_tail(logged: &str) -> Vec<String> {
    let lines: Vec<&str> = logged.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(LOG_TAIL)..].iter().map(|line| crate::foundation::redact::redact(line)).collect()
}

fn health_check(layout: &Layout) -> Command {
    let mut command = Command::new(layout.current().join("north"));
    command.args(["runtime", "health"]);
    command
}

fn agents_sync(north: &Path) -> Command {
    let mut command = Command::new("agents");
    command.arg("sync").env("NORTH_BIN", north);
    command
}

pub(in crate::runtime) fn alert_proxy(text: &str) {
    let sent = Command::new("north")
        .args(["fleet", "send", "--from", "service:north-runtime-update", "proxy", text])
        .status();
    if !sent.is_ok_and(|status| status.success()) {
        eprintln!("north runtime update: rollback alert delivery failed");
    }
}
