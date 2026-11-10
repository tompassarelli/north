use std::io::IsTerminal;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::{Fleet, backlog_line};
use crate::fleet::runtime::{PROXY, now_ms};
use crate::fleet::store::{Addressee, Store};
use crate::threads::client;
use crate::tui;

use super::interactive;
use super::placement::Overrides;
use super::revive;
use super::router;
use super::window_move;

const USAGE: &str = "usage: north proxy [--interactive] | north proxy move [--to ACCOUNT] [--auto] [--dry-run]";
const OPENING: &str = "[north: north proxy started this session as Tom's proxy]";
const ROLE_FILE: &str = "harness/claude/agents/proxy.md";
/// Why a launch names its model outside the tier routing: the proxy takes its model and effort from the role file, and
/// the record `north fleet show proxy` prints comes from the launch request alone.
const TIER_REASON: &str = "north#1048: the proxy runs the model and effort harness/claude/agents/proxy.md gives it";

pub(in crate::cli) fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["move", rest @ ..] => window_move::run(rest),
        [] => router::open_fleet().and_then(attach),
        ["--interactive"] => router::open_fleet().and_then(|fleet| interactive::run(&fleet, PROXY, false)),
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
    match fleet.fleet_proxy()? {
        Some(record) if fleet.running(&record) => println!("north proxy: {PROXY} is live on account {}", record.account),
        Some(_) => {
            revive::revive(&fleet, PROXY, None, None, Overrides::default())?;
            println!("north proxy: revived {PROXY}");
        }
        None => launch(&fleet)?,
    }
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }
    tui::fleet::run_on(fleet, PROXY, crate::cli::dash::for_fleet, crate::cli::glance::lines)
}

fn launch(fleet: &Fleet) -> NorthResult<()> {
    let earlier = fleet.earlier_proxies()?;
    let opening = if earlier.is_empty() {
        OPENING.to_owned()
    } else {
        let addressees = earlier.iter().map(|record| fleet.addressee(record, 0).map(|owed| Addressee { stopped: false, ..owed })).collect::<NorthResult<Vec<_>>>()?;
        let rows = Store::open(&client::ledger_path()?)?.addressed_to(&addressees)?;
        let mut names: Vec<String> = earlier.iter().map(|record| record.name.clone()).collect();
        names.sort();
        for name in &names {
            let archived = fleet.retire_proxy(name)?;
            eprintln!("north proxy: archived {name} at {}", archived.display());
        }
        backlog_line(&names, &rows, now_ms())
    };
    let main = home()?.join("code/north/main");
    let words = launch_words(&main.to_string_lossy(), &opening, definition());
    router::launch(fleet, &words.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The words `north fleet launch` starts a fresh proxy with: the role file's model and effort, so the record
/// `north fleet show proxy --json` prints names the model the session runs, which Claude Code otherwise takes
/// from the agent definition alone and north's record never sees.
fn launch_words(main: &str, opening: &str, (model, effort): (Option<String>, Option<String>)) -> Vec<String> {
    let mut words: Vec<String> = ["--provider", "claude", "--role", "proxy", "--parent", "none", "--cwd", main].map(str::to_owned).to_vec();
    words.extend(model.into_iter().flat_map(|model| ["--model".to_owned(), model, "--tier-reason".to_owned(), TIER_REASON.to_owned()]));
    words.extend(effort.into_iter().flat_map(|effort| ["--effort".to_owned(), effort]));
    words.extend(["--take-over".to_owned(), opening.to_owned()]);
    words
}

/// The model and effort the proxy's role file names, read from the harness tree of the runtime running this command:
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
#[path = "proxy_tests.rs"]
mod tests;
