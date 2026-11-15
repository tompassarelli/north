use std::io::IsTerminal;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Surface;
use crate::fleet::registry::{Fleet, backlog_line};
use crate::fleet::runtime::{COORDINATOR, now_ms};
use crate::fleet::store::Addressee;
use crate::tui;

use super::interactive;
use super::placement::Overrides;
use super::revive;
use super::router;
use super::coordinator_move;

const USAGE: &str = "usage: north coordinator [--interactive] | north coordinator move [--to ACCOUNT] [--effort LEVEL] [--auto]";
const OPENING: &str = "[north: north coordinator started this session as Tom's coordinator]";
const ROLE_FILE: &str = "harness/claude/agents/coordinator.md";
/// Why a launch names its model outside the tier routing: the coordinator takes its model and effort from the role file, and
/// the record `north fleet show coordinator` prints comes from the launch request alone.
const TIER_REASON: &str = "north#1048: the coordinator runs the model and effort harness/claude/agents/coordinator.md gives it";

pub(in crate::cli) fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["move", rest @ ..] => coordinator_move::run(rest),
        [] => router::open_fleet().and_then(attach),
        ["--interactive"] => router::open_fleet().and_then(|fleet| interactive::run(&fleet, COORDINATOR, false)),
        _ => Err(NorthError::Usage(USAGE.into())),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            1
        }
    }
}

fn attach(fleet: Fleet) -> NorthResult<()> {
    match fleet.fleet_coordinator()? {
        Some(record) if fleet.running(&record) => println!("north coordinator: {COORDINATOR} is live on account {}", record.account),
        Some(_) => {
            fleet.hold_coordinator_on(COORDINATOR, surface(&fleet))?;
            revive::revive(&fleet, COORDINATOR, None, None, Overrides::default())?;
            println!("north coordinator: revived {COORDINATOR}");
        }
        None => launch(&fleet, surface(&fleet))?,
    }
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }
    tui::fleet::run_on(fleet, COORDINATOR, super::reads::reads())
}

fn surface(fleet: &Fleet) -> Surface {
    match fleet.coordinator_window_closed() {
        Some(why) => {
            eprintln!("north coordinator: no window opened for {COORDINATOR}, so the fleet screen holds it: {why}");
            Surface::Screen
        }
        None => Surface::Window,
    }
}

fn launch(fleet: &Fleet, surface: Surface) -> NorthResult<()> {
    if crate::pacing::placement::gate::live_gate()["claude"]["windows"].as_object().map_or(0, serde_json::Map::len) == 0 {
        crate::cli::agents::with_usage_host(crate::usage::refresh).map_err(|error| NorthError::State(format!("the usage reading failed: {error}")))?;
    }
    let earlier = fleet.earlier_proxies()?;
    let opening = if earlier.is_empty() {
        OPENING.to_owned()
    } else {
        let addressees = earlier.iter().map(|record| fleet.addressee(record, 0).map(|owed| Addressee { stopped: false, ..owed })).collect::<NorthResult<Vec<_>>>()?;
        let rows = super::ledger::addressed(&addressees)?;
        let mut names: Vec<String> = earlier.iter().map(|record| record.name.clone()).collect();
        names.sort();
        for name in &names {
            let archived = fleet.retire_coordinator(name)?;
            eprintln!("north coordinator: archived {name} at {}", archived.display());
        }
        backlog_line(&names, &rows, now_ms())
    };
    let main = home()?.join("code/north/main");
    let mut words = launch_words(&main.to_string_lossy(), &opening, definition());
    if surface == Surface::Screen {
        words.insert(0, "--screen".to_owned());
    }
    router::launch(fleet, &words.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The words `north fleet launch` starts a fresh coordinator with: the role file's model and effort, so the record
/// `north fleet show coordinator --json` prints names the model the session runs, which Claude Code otherwise takes
/// from the agent definition alone and north's record never sees.
fn launch_words(main: &str, opening: &str, (model, effort): (Option<String>, Option<String>)) -> Vec<String> {
    let mut words: Vec<String> = ["--provider", "claude", "--role", "coordinator", "--parent", "none", "--cwd", main].map(str::to_owned).to_vec();
    words.extend(model.into_iter().flat_map(|model| ["--model".to_owned(), model, "--tier-reason".to_owned(), TIER_REASON.to_owned()]));
    words.extend(effort.into_iter().flat_map(|effort| ["--effort".to_owned(), effort]));
    words.extend(["--take-over".to_owned(), opening.to_owned()]);
    words
}

/// The model and effort the coordinator's role file names, read from the harness tree of the runtime running this command:
/// the same two lines Claude Code reads as the session's agent definition.
fn definition() -> (Option<String>, Option<String>) {
    crate::platform::harness::harness_root().ok().and_then(|root| std::fs::read_to_string(root.join(ROLE_FILE)).ok()).map_or((None, None), |text| fields(&text))
}

/// The `model:` and `effort:` of a role file's frontmatter, absent when it names neither.
fn fields(text: &str) -> (Option<String>, Option<String>) {
    let head = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---")).map_or("", |(head, _)| head);
    let field = |name: &str| head.lines().filter_map(|line| line.split_once(':')).find(|(key, _)| *key == name).map(|(_, value)| value.trim().to_owned()).filter(|value| !value.is_empty());
    (field("model"), field("effort"))
}

fn home() -> NorthResult<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
