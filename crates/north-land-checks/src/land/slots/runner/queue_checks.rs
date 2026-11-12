use std::path::Path;

use super::super::clippy_first;
use super::super::verdict_key;
use super::{Arguments, Config, affected, block_on, run_added_avoiding, say, shared_build, steps, warn};
use north_base::foundation::exit::{NO_VERDICT_STATUS, SLOTS_BUSY_STATUS};

const ATTEMPTS: usize = 3;

/// The checks a lane runs alone before its ticket is written: its clippy (build and link), the
/// `@enqueue` land.check lines, and the land test selection the batch would otherwise run, or the
/// land.check lines themselves when none of them reads a test selection, so a lane is never vouched
/// for on tests that did not run (north#1230). Read without running them, so a test can pin the set
/// (north#892).
pub fn queue_commands(root: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let config = Config::load(root, head);
    let landing = config.after_set().commands(root, base, head)?;
    let clippy = clippy_first::listed(&landing, |command| !config.is_inferred(command));
    let queued_lines = config.queue_set(crate::landing_refusals::enforcing()).commands(root, base, head)?;
    let queued: Vec<String> = queued_lines.iter().map(|command| shared_build::built(command)).collect();
    let tests = match lane_tests(&landing) {
        Some(selected) => vec![selected],
        None => config.commands(root, base, head)?.iter().map(|command| shared_build::built(command)).collect(),
    };
    let mut commands: Vec<String> = clippy.into_iter().collect();
    for command in queued.into_iter().chain(tests) {
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    Ok(commands)
}

pub fn refuse_queue_checks(root: &Path, base: &str, head: &str) -> Result<(), String> {
    steps::begin();
    let (config, clippy, commands) = steps::timed("commands", || -> Result<_, String> {
        let config = Config::load(root, head);
        let clippy = clippy_first::listed(&config.after_set().commands(root, base, head)?, |command| !config.is_inferred(command));
        Ok((config, clippy, queue_commands(root, base, head)?))
    })?;
    if commands.is_empty() {
        return Ok(());
    }
    say(&format!("safe-push: running {} check(s) before queueing the lane: {}", commands.len(), commands.join("; ")));
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let mut failed: Vec<String> = Vec::new();
    let mut status = NO_VERDICT_STATUS;
    let mut attempt = 1;
    while attempt <= ATTEMPTS {
        let ran = block_on(run_added_avoiding(root.to_owned(), arguments.clone(), config.clone(), commands.clone(), &failed)).map_err(|error| error.to_string())?;
        status = ran.status;
        if ran.unanswered {
            return Err("no build host answers over ssh, so the checks before queueing cannot run; the lane is not queued uncompiled, so run safe-push again once a build host is back".to_owned());
        }
        if status == SLOTS_BUSY_STATUS {
            say("safe-push: every slot on the build host stayed busy for its wait limit; the checks before queueing wait in line again");
            continue;
        }
        if status != NO_VERDICT_STATUS {
            break;
        }
        if attempt < ATTEMPTS {
            warn(&format!("safe-push: the checks before queueing reached no verdict on {}; running them again ({} of {ATTEMPTS})", if ran.host.is_empty() { "this host" } else { ran.host.as_str() }, attempt + 1));
        }
        if !ran.host.is_empty() {
            failed.push(ran.host);
        }
        attempt += 1;
    }
    if status == 0 {
        return Ok(());
    }
    if status == NO_VERDICT_STATUS {
        return Err(format!("the checks before queueing reached no verdict {ATTEMPTS} times (exit 75: the host or its connection failed before they reported); the lane is not queued uncompiled, so run safe-push again"));
    }
    let rerun = match clippy {
        Some(_) => {
            let crates = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.changed_arguments());
            format!("; to rerun clippy alone: cargo clippy --all-targets {crates} -- -D warnings")
        }
        None => String::new(),
    };
    Err(format!("a check before queueing exited {status}; fix the failure above{rerun}"))
}

fn lane_tests(landing: &[String]) -> Option<String> {
    landing.iter().find(|command| verdict_key::reads_it(std::slice::from_ref(command))).cloned()
}
