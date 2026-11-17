//! `north cloud`: the command surface over the sessions API and the cloud landing. It parses the
//! arguments, keeps the collect and land paths their own, and executes the API
//! commands here, leaving the transport to `api` and a turn's lifecycle to `turn`.
use std::io::Write;
use std::path::{Path, PathBuf};

mod api;
mod turn;

pub(super) use api::{Api, events, save};

use api::{credentials, pages, row};
use turn::{Finish, NUDGES, POLL, Waker, finish, send, sweep};

pub(super) const USAGE: &str = "usage: north cloud list | events SESSION_ID OUT.jsonl | sweep OUT_DIR | send SESSION_ID FILE | wait SESSION_ID OUT.jsonl [--after UUID] [--timeout SECS] | collect EVENTS.jsonl OUT.mbox | land EVENTS.jsonl [--repo DIR]
 sweep archives only cloud-run sessions (environment_kind anthropic_cloud) waiting on a human, never Remote Control bridges.
 send posts FILE as a user message to the session and prints the HTTP status and the message's uuid.
 wait returns once the session is idle and its latest turn ended, after the message UUID when --after is given, then writes its events (default timeout 3600 s; exit 1 on timeout); a turn that ends with no Done:/Not done:/Split:/Blocked: line and no patch handback gets a continue message posted and is waited on again, at most twice, before wait reports the worker stalled and exits 1; when those events hold a patch handback it then lands it as land does.
 collect decodes the latest multi-part patch handback in a saved events transcript to an mbox, reading no session and needing no login; a missing part, a failed decode or output not starting with From is refused.
 land collects the handback, applies it with git am --3way in a fresh worktrees/cloud-SESSION lane off origin/main, folds it into one commit without the session trailers, runs north land checks on the lane and enqueues it with safe-push --to main, printing Done: queued; a patch that does not apply or a failed check stops with the reason and the lane path and queues nothing, and a handback already queued is a no-op.
 Run on the host whose ~/.claude login owns the cloud sessions (south:agents/skills/claude-accounts/).";

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Cloud {
    List,
    Events(String, PathBuf),
    Sweep(PathBuf),
    Send(String, PathBuf),
    Wait(String, PathBuf, u64, Option<String>),
    Collect(PathBuf, PathBuf),
    Land(PathBuf, PathBuf),
}

pub(super) fn parse(arguments: &[String]) -> Option<Cloud> {
    let words = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    Some(match words.as_slice() {
        ["list"] => Cloud::List,
        ["events", id, out] => Cloud::Events((*id).to_owned(), out.into()),
        ["collect", events, out] => Cloud::Collect(events.into(), out.into()),
        ["land", events] => Cloud::Land(events.into(), ".".into()),
        ["land", events, "--repo", repo] => Cloud::Land(events.into(), repo.into()),
        ["sweep", dir] => Cloud::Sweep(dir.into()),
        ["send", id, file] => Cloud::Send((*id).to_owned(), file.into()),
        ["wait", id, out, options @ ..] => wait_command(id, out, options)?,
        _ => return None,
    })
}

fn wait_command(id: &str, out: &str, options: &[&str]) -> Option<Cloud> {
    let (mut after, mut timeout) = (None, 3600);
    for pair in options.chunks(2) {
        match pair {
            ["--after", uuid] => after = Some((*uuid).to_owned()),
            ["--timeout", secs] => timeout = secs.parse().ok()?,
            _ => return None,
        }
    }
    Some(Cloud::Wait(id.to_owned(), out.into(), timeout, after))
}

pub(super) fn execute(command: Cloud, api: &mut Api, out: &mut dyn Write) -> Result<u8, String> {
    match command {
        Cloud::List => {
            for session in pages(api, "/v1/sessions?beta=true&limit=100")? {
                let _ = writeln!(out, "{}", row(&session));
            }
        }
        Cloud::Events(id, path) => {
            let events = events(api, &id)?;
            save(&events, &path)?;
            let _ = writeln!(out, "{} events in {}", events.len(), path.display());
        }
        Cloud::Sweep(dir) => sweep(api, &dir, out)?,
        Cloud::Collect(..) => {
            return Err("collect reads a saved transcript, not the sessions API".into());
        }
        Cloud::Land(..) => return Err("land reads a saved transcript, not the sessions API".into()),
        Cloud::Send(id, file) => {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            let uuid = new_uuid()?;
            let status = send(api, &id, &uuid, &text)?;
            let _ = writeln!(out, "{status} {uuid}");
            return Ok(if (200..300).contains(&status) { 0 } else { 1 });
        }
        Cloud::Wait(id, path, timeout, after) => {
            let waker = Waker {
                polls: timeout / POLL.as_secs(),
                sleep: &mut || std::thread::sleep(POLL),
                uuid: &mut || {
                    new_uuid()
                },
                report: &mut *out,
            };
            match finish(api, &id, &path, after.as_deref(), waker)? {
                Finish::Ended(count) => {
                    let _ = writeln!(
                        out,
                        "{id} finished its turn; {count} events in {}",
                        path.display()
                    );
                }
                Finish::Stalled(count) => {
                    let _ = writeln!(
                        out,
                        "{id} stalled: its turn still ended with no report or handback after {NUDGES} continue messages; {count} events in {}",
                        path.display()
                    );
                    return Ok(1);
                }
                Finish::TimedOut => {
                    let _ = writeln!(out, "{id} did not finish its turn within {timeout} s");
                    return Ok(1);
                }
            }
        }
    }
    Ok(0)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(command) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if let Cloud::Collect(events, out) = &command {
        return match super::cloud_handback::run(events, out, &mut std::io::stdout()) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north cloud collect: {error}");
                1
            }
        };
    }
    if let Cloud::Land(events, repo) = &command {
        return super::cloud_land::run(events, repo, false);
    }
    let landing = match &command {
        Cloud::Wait(id, path, ..) => Some((id.clone(), path.clone())),
        _ => None,
    };
    let result = credentials().and_then(|curl| {
        execute(
            command,
            &mut |method, path, body| curl.call(method, path, body),
            &mut std::io::stdout(),
        )
    });
    match (result, landing) {
        (Ok(0), Some((_id, path))) => super::cloud_land::run(&path, Path::new("."), true),
        (Ok(code), _) => code,
        (Err(error), _) => {
            eprintln!("north cloud: {error}");
            1
        }
    }
}

fn new_uuid() -> Result<String, String> {
    std::fs::read_to_string("/proc/sys/kernel/random/uuid").map(|uuid| uuid.trim().to_owned()).map_err(|error| error.to_string())
}
