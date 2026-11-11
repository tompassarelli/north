use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Instant;
use north_base::foundation::exit::NO_VERDICT_STATUS;
use north_base::foundation::offload::OWNER_ENV;

use super::runner::{self, Ended, ended, supervise};
use crate::land::outlive::SLICE_ENV;
use super::{Landed, Run, superseded};
use crate::landing::red_check::{self, Failure};
use crate::land::candidate::change_worktrees_alone;
use crate::land::checks::Verdict;
use north_base::foundation::exit::SLOTS_BUSY_STATUS;
use crate::land::telemetry;

pub(crate) struct Checked {
    pub(super) verdict: Verdict,
    pub(super) expired: bool,
}

impl From<Verdict> for Checked {
    fn from(verdict: Verdict) -> Self {
        Self { verdict, expired: false }
    }
}

pub(super) fn check(landed: &Landed, tree: &str, run: &Run) -> Checked {
    if run.superseded() {
        return superseded(landed).into();
    }
    let Some((program, prefix)) = run.runner.split_first() else { return Verdict::Transient("no post-land runner".to_owned()).into() };
    if let Err(error) = clear(&landed.repo, &run.worktree) {
        return Verdict::Transient(format!("post-land worktree: {} left by an earlier run could not be removed: {error}", run.worktree.display())).into();
    }
    if let Err(error) = change_worktrees_alone(&landed.repo, &["worktree", "add", "--detach", &run.worktree.to_string_lossy(), tree]) {
        return Verdict::Transient(format!("post-land worktree: {error}")).into();
    }
    let checked = execute(program, prefix, landed, tree, run);
    let _ = change_worktrees_alone(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    checked
}

fn clear(repo: &Path, worktree: &Path) -> std::io::Result<()> {
    let _ = change_worktrees_alone(repo, &["worktree", "remove", "--force", &worktree.to_string_lossy()]);
    match fs::remove_dir_all(worktree) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let _ = change_worktrees_alone(repo, &["worktree", "prune"]);
    Ok(())
}

pub(crate) fn execute(program: &str, prefix: &[String], landed: &Landed, tree: &str, run: &Run) -> Checked {
    if let Some(parent) = run.log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Verdict::Transient(format!("{}: {error}", parent.display())).into();
    }
    let _ = fs::remove_file(&run.outcome);
    let (stdout, stderr) = match File::create(&run.log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Verdict::Transient(format!("{}: {error}", run.log.display())).into(),
    };
    let here = run.host.as_deref() == Some(north_base::platform::host::name().as_str()) && !run.capacity.is_empty();
    let mut argv: Vec<OsString> = Vec::new();
    if here {
        argv.extend(["bash".into(), "-euo".into(), "pipefail".into(), "-c".into(), format!("exec {} \"$@\"", run.capacity).into(), "post-land-capacity".into()]);
    }
    argv.push(program.into());
    argv.extend(prefix.iter().map(OsString::from));
    argv.extend(["--base", &landed.base, "--head", &landed.head, "--tree", tree].map(OsString::from));
    argv.extend(crate::land::blame::ranges(&landed.base, &landed.lanes, &landed.tips).into_iter().flat_map(|lane| ["--lane".into(), lane.into()]));
    argv.extend(landed.ran.iter().flat_map(|pass| ["--ran".into(), pass.into()]));
    argv.extend(["--outcome".into(), run.outcome.clone().into_os_string()]);
    if run.adoptable {
        argv = runner::wrapped(run, argv);
    }
    let Some((first, rest)) = argv.split_first() else { return Verdict::Transient("no post-land runner".to_owned()).into() };
    let mut command = Command::new(first);
    command.args(rest).current_dir(&run.worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env(OWNER_ENV, &run.offload_owner).env("SAFE_PUSH_LAND_CAPACITY", &run.capacity);
    match &run.host {
        Some(host) => command.env("SAFE_PUSH_LAND_RUNNER", host),
        None => command.env_remove("SAFE_PUSH_LAND_RUNNER"),
    };
    if let Some(logs) = run.log.parent() {
        command.env(crate::land::trace::SPANS_ENV, crate::land::trace::Spans::beside(logs).path());
    }
    for name in crate::land::checks::SCRUBBED {
        command.env_remove(name);
    }
    command.env_remove(SLICE_ENV);
    command.process_group(0);
    let supervised = command.spawn().and_then(|mut child| {
        let Some(group) = rustix::process::Pid::from_raw(child.id().cast_signed()) else { return Err(std::io::Error::other("the runner has no process id")) };
        if run.adoptable {
            runner::remember(run, landed, child.id());
        }
        let supervised = supervise(group, Instant::now() + run.deadline, &run.cancel, || child.try_wait());
        if !matches!(supervised, Ok(Ended::Exited(_))) {
            let _ = child.kill();
            let _ = child.wait();
        }
        if run.adoptable {
            runner::forget(run);
        }
        supervised
    });
    ended(supervised, program, landed, run)
}

pub(super) fn judged(status: ExitStatus, run: &Run) -> Verdict {
    match status.code() {
        _ if status.success() => Verdict::Green,
        Some(126 | 127) => Verdict::Transient(format!("post-land checks could not start their command (exit {status}), so they reached no verdict; log: {}", run.log.display())),
        Some(NO_VERDICT_STATUS) => Verdict::Transient(format!("post-land checks exited {NO_VERDICT_STATUS}; log: {}", run.log.display())),
        Some(SLOTS_BUSY_STATUS) => Verdict::Transient(format!("every build-host slot stayed busy, so the post-land checks reached no verdict; log: {}", run.log.display())),
        _ if fs::read_to_string(&run.log).ok().and_then(|text| north_base::foundation::check_log::ref_lock(&text)).is_some() => Verdict::Transient(format!("post-land checks hit a git ref-lock failure, an infrastructure fault, so they reached no verdict; log: {}", run.log.display())),
        _ if steps(&run.outcome).is_empty() => Verdict::Transient(format!("post-land checks exited {status} with no check reaching a failing verdict, so they reached no verdict; log: {}", run.log.display())),
        _ => Verdict::RedMain(failed_check(&run.outcome).unwrap_or_else(|| format!("exit {status}; log: {}", run.log.display()))),
    }
}

pub(crate) fn steps(outcome: &Path) -> BTreeSet<Failure> {
    let text = fs::read_to_string(outcome).unwrap_or_default();
    text.lines().filter_map(|line| line.strip_prefix("failed-step\t")).filter_map(|rest| rest.split('\t').next()).filter(|command| !command.is_empty()).map(|command| Failure::Step(command.to_owned())).collect()
}

pub(crate) fn failing(log: &Path, outcome: &Path) -> BTreeSet<Failure> {
    let mut failing = red_check::failures(&fs::read_to_string(log).unwrap_or_default());
    failing.extend(steps(outcome));
    failing
}

pub(super) fn failed_check(outcome: &Path) -> Option<String> {
    let text = fs::read_to_string(outcome).ok()?;
    let failed: Vec<String> = text.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        (fields.first() == Some(&"event") && fields.get(2) == Some(&"failed")).then(|| fields.get(3).copied().unwrap_or_default().to_owned())
    }).collect();
    (!failed.is_empty()).then(|| failed.join("; "))
}

pub(super) fn record(run: &Run, landed: &Landed, verdict: &Verdict, expired: bool, seconds: u64) {
    if matches!(verdict, Verdict::Transient(_)) && !expired {
        return;
    }
    let (class, detail) = verdict.class();
    if let Some(parent) = run.ledger.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tests = fs::read_to_string(&run.log).ok().and_then(|text| test_run_seconds(&text)).map(|seconds| seconds.to_string()).unwrap_or_default();
    let covered_by = |head: &str| if head == landed.head { "" } else { landed.head.as_str() };
    let lines: String = landed.heads.iter().map(|head| format!("{}\t{}\t{head}\t{class}\t{}\t{}\t{seconds}\t{}\t{tests}\n", telemetry::seconds(), landed.repository, detail.replace(['\t', '\n'], " "), landed.lanes.join(","), covered_by(head))).collect();
    if let Err(error) = OpenOptions::new().create(true).append(true).open(&run.ledger).and_then(|mut file| file.write_all(lines.as_bytes())) {
        eprintln!("north land: could not record the post-land verdict of {}: {error}", landed.head);
    }
}

pub(super) fn test_run_seconds(log: &str) -> Option<u64> {
    let runs: Vec<f64> = log.lines().filter_map(|line| line.trim_start().strip_prefix("Summary [")?.split_once("s]")?.0.trim().parse().ok()).collect();
    (!runs.is_empty()).then(|| runs.iter().sum::<f64>().round() as u64)
}
