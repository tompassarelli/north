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
const AGENTS_SYNC: &str = "agents sync";
const UNPUBLISHED: &str = "agents-sync-unpublished";

pub(super) fn swap(layout: &Layout, link: &Path) -> std::io::Result<()> {
    let staged = layout.state.join("current.tmp");
    let _ = fs::remove_file(&staged);
    symlink(link, &staged)?;
    fs::rename(&staged, layout.current())
}

pub(super) fn sync(layout: &Layout, previous: Option<&Path>, target: &str) -> Result<String, Failure> {
    let steps = vec![("health check", health_check(layout)), (AGENTS_SYNC, agents_sync(&layout.runtime(target).join("north")))];
    settle(layout, previous, target, steps, &mut |text: &str| alert_proxy(&format!("north runtime update: {text}")))
}

pub(super) fn republish_if_unpublished(layout: &Layout, target: &str) {
    if !layout.state.join(UNPUBLISHED).exists() {
        return;
    }
    let steps = vec![(AGENTS_SYNC, agents_sync(&layout.runtime(target).join("north")))];
    if let Ok(synced) = settle(layout, None, target, steps, &mut |text: &str| alert_proxy(&format!("north runtime update: {text}"))) {
        print!("{synced}");
    }
}

pub(super) fn settle(layout: &Layout, previous: Option<&Path>, target: &str, steps: Vec<(&str, Command)>, notify: &mut dyn FnMut(&str)) -> Result<String, Failure> {
    let log = tempfile::Builder::new()
        .prefix(&format!("runtime-sync-{}.", &target[..8]))
        .suffix(".log")
        .tempfile_in(&layout.state)?;
    swap(layout, &layout::link(target))?;
    let (failure, unpublished) = run_steps(steps, log.path());
    let Some((step, status)) = failure else {
        let logged = fs::read_to_string(log.path()).unwrap_or_default();
        publication_notice(layout, target, unpublished.map(|status| (status, logged.as_str())), notify);
        return Ok(logged.lines().map(|line| crate::foundation::redact::redact(line) + "\n").collect());
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

fn run_steps<'a>(steps: Vec<(&'a str, Command)>, log: &Path) -> (Option<(&'a str, u8)>, Option<u8>) {
    let mut failure = None;
    let mut unpublished = None;
    let limit = remote::limit(STEP_LIMIT);
    let append = || fs::OpenOptions::new().append(true).open(log);
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
            Ok(status) if name == AGENTS_SYNC => {
                unpublished = Some(code(status));
                continue;
            }
            Err(error) if name == AGENTS_SYNC => {
                note(format!("{name} did not run: {error}\n"));
                unpublished = Some(127);
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
    (failure, unpublished)
}

fn publication_notice(layout: &Layout, target: &str, unpublished: Option<(u8, &str)>, notify: &mut dyn FnMut(&str)) {
    let record = layout.state.join(UNPUBLISHED);
    let Some((status, logged)) = unpublished else {
        let _ = fs::remove_file(&record);
        return;
    };
    let cause = match status {
        POLICY_HELD => logged
            .lines()
            .find(|line| line.starts_with("agents sync: ") && !line.contains("generation not published"))
            .unwrap_or("agents sync: the policy gate refused the generation")
            .to_owned(),
        _ => format!("agents sync exited {status}: {}", log_tail(logged).last().map_or("it printed nothing", String::as_str)),
    };
    if fs::read_to_string(&record).is_ok_and(|seen| seen == cause) {
        return;
    }
    let _ = fs::write(&record, &cause);
    let short = &target[..8];
    match status {
        POLICY_HELD => notify(&format!("runtime {short} is live; the policy generation is held and the last published one stays live: {cause}")),
        _ => notify(&format!("runtime {short} is live; the harness generation is not published and the last published one stays live until agents sync passes on a later runtime update tick: {cause}")),
    }
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
        eprintln!("north runtime update: proxy alert delivery failed");
    }
}
