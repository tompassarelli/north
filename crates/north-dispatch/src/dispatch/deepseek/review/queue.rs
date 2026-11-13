//! Queueing one review job per Done report: on the laptop through its job queue, here, or on the host that holds the
//! session over ssh.

use super::done::{mark, Done, Found};
use super::super::super::record;
use crate::pacing::placement;
use std::process::Command;

/// The `north` words of the review job for `done`.
pub fn job(done: &Done) -> Vec<String> {
    let mut words: Vec<String> = ["fleet", "dispatch", "review", &done.name, "--item", &done.item.to_string(), "--category", &done.category].map(str::to_owned).into();
    if let Some(text) = &done.box_text {
        words.extend(["--box".to_owned(), text.clone()]);
    }
    if done.landing {
        words.push("--landing".into());
    }
    for test in &done.oracle {
        words.extend(["--oracle".to_owned(), test.clone()]);
    }
    words
}

/// Runs `north WORDS` on a host other than this one and the laptop.
pub type Remote<'a> = dyn Fn(&str, &[String]) -> Result<(), String> + 'a;

/// How many review jobs one dispatch run queues, the oldest Done report first, so a backlog reaches the build hosts a few at a time.
pub const PER_RUN: usize = 3;

/// Queues the review job of the `PER_RUN` oldest sessions in `found`: on the laptop through its job queue, here by running
/// it, on any other host by running it there over ssh; records each retired session it leaves alone; prints one line each
/// and the counts.
pub fn queue(found: &Found, here: &str, laptop: Option<&str>, remote: &Remote<'_>, dry_run: bool) -> Vec<String> {
    let north = std::env::current_exe().map_or_else(|_| "north".into(), |path| path.to_string_lossy().into_owned());
    let directory = record::directory();
    let would = if dry_run { "would " } else { "" };
    let mut lines: Vec<String> = found
        .left
        .iter()
        .map(|left| {
            let line = format!("{would}leave {} on {}: {}", left.name, left.host, left.why);
            if dry_run || !left.retired {
                return line;
            }
            match mark(&directory, &left.name, placement::now(), Some(&left.why), None) {
                Ok(()) => line,
                Err(why) => format!("{line}; not recorded: {why}"),
            }
        })
        .collect();
    lines.extend(found.done.iter().take(PER_RUN).map(|done| {
        let line = format!("review {} on {} ({}, {})", done.name, done.host, done.item, done.category);
        if dry_run {
            return format!("would {line}");
        }
        let ran = start(done, &north, here, laptop, remote);
        match ran.and_then(|()| mark(&directory, &done.name, placement::now(), None, None)) {
            Ok(()) => line,
            Err(why) => match mark(&directory, &done.name, placement::now(), None, Some(&why)) {
                Ok(()) => format!("{line} failed, retried in an hour so the sessions behind it go first: {why}"),
                Err(unrecorded) => format!("{line} failed: {why}; not recorded: {unrecorded}"),
            },
        }
    }));
    if !found.done.is_empty() || !found.left.is_empty() {
        let queued = found.done.len().min(PER_RUN);
        lines.push(format!("review: {would}queue {queued} of {} waiting, {} left for later runs at {PER_RUN} a run; {would}leave {}", found.done.len(), found.done.len() - queued, found.left.len()));
    }
    lines
}

/// Starts `done`'s review job on the host that holds it: through the laptop's job queue, here, or over ssh.
fn start(done: &Done, north: &str, here: &str, laptop: Option<&str>, remote: &Remote<'_>) -> Result<(), String> {
    if laptop == Some(done.host.as_str()) {
        let mut queued = ["threads", "queue", "add", "laptop", &done.item.to_string(), "--", "north"].map(str::to_owned).to_vec();
        queued.extend(job(done));
        run(north, &queued)
    } else if done.host == here {
        run(north, &job(done))
    } else {
        remote(&done.host, &job(done))
    }
}

fn run(north: &str, words: &[String]) -> Result<(), String> {
    let output = Command::new(north).args(words).output().map_err(|error| error.to_string())?;
    if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}
