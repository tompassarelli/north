use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::github_app::{self, AGENT_APP, Host, Request};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north gh-session REAL_GH [ARGS...]

The gh a fleet session runs through the bin/gh shim: it calls GitHub as the Socrates Agent App
(a fresh installation token in GH_TOKEN, never Tom's login) and spends from a per-session hourly
budget of GitHub API units, so one session's scraping cannot exhaust a token the others share.
A call costs 1 unit, --paginate 20, --limit N one per 100 rows; help, version and config cost none.
A call past the budget is refused with exit 75 and a message naming the spend and when it frees.
Budget: ${NORTH_GH_HOURLY_BUDGET:-400} units per rolling hour, counted in
${XDG_RUNTIME_DIR}/north-gh-budget/SESSION. Outside a fleet session (no NORTH_FLEET_SESSION)
REAL_GH runs unchanged.";

const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
const BUDGET_ENV: &str = "NORTH_GH_HOURLY_BUDGET";
const DEFAULT_BUDGET: u32 = 400;
const WINDOW_SECONDS: i64 = 3600;
const PAGINATE_COST: u32 = 20;
const ROWS_PER_UNIT: u32 = 100;
const FREE_COMMANDS: [&str; 6] = ["help", "version", "config", "alias", "completion", "extension"];
const SCRUBBED: [&str; 4] = ["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"];
const REFUSED: u8 = 75;

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some((real, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 1;
    };
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty());
    let Some(session) = session else {
        return exec(Command::new(real).args(rest));
    };
    let budget = std::env::var(BUDGET_ENV).ok().and_then(|value| value.parse().ok()).unwrap_or(DEFAULT_BUDGET);
    let ledger = ledger_path(&session);
    let mint = || {
        let request = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}"))?;
        github_app::mint(&request, &Host::from_env(), Timestamp::now().unix_seconds())
    };
    match prepare(real, rest, &session, budget, &ledger, Timestamp::now().unix_seconds(), mint) {
        Ok(command) => exec(command),
        Err(Refusal::Budget(message)) => {
            eprintln!("north gh-session: {message}");
            REFUSED
        }
        Err(Refusal::Token(message)) => {
            eprintln!("north gh-session: no Socrates Agent App token ({message}); not falling back to a personal login");
            1
        }
    }
}

fn exec(mut command: impl std::borrow::BorrowMut<Command>) -> u8 {
    let command = command.borrow_mut();
    let error = command.exec();
    eprintln!("north gh-session: cannot run {}: {error}", command.get_program().to_string_lossy());
    127
}

fn ledger_path(session: &str) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let name: String = session.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') { character } else { '_' }).collect();
    runtime.join("north-gh-budget").join(name)
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Budget(String),
    Token(String),
}

fn prepare(real: &str, arguments: &[String], session: &str, budget: u32, ledger: &Path, now: i64, mint: impl FnOnce() -> Result<String, String>) -> Result<Command, Refusal> {
    let units = cost(arguments);
    if units > 0 {
        admit(&fresh_spend(ledger, now), now, units, budget, session).map_err(Refusal::Budget)?;
    }
    let token = mint().map_err(Refusal::Token)?;
    if units > 0 {
        record(ledger, now, units);
    }
    let mut command = Command::new(real);
    command.args(arguments);
    for name in SCRUBBED {
        command.env_remove(name);
    }
    command.env("GH_TOKEN", token).env("GH_PROMPT_DISABLED", "1");
    Ok(command)
}

fn cost(arguments: &[String]) -> u32 {
    let help = arguments.iter().any(|word| matches!(word.as_str(), "-h" | "--help" | "--version"));
    if help || arguments.first().is_none_or(|word| FREE_COMMANDS.contains(&word.as_str())) {
        return 0;
    }
    let rows = |text: &str| text.parse::<u32>().ok().map(|rows| rows.div_ceil(ROWS_PER_UNIT));
    let mut cost = 1;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let extra = match word.as_str() {
            "--paginate" => Some(PAGINATE_COST),
            "--limit" | "-L" => words.next().and_then(|value| rows(value)),
            other => other.strip_prefix("--limit=").and_then(rows),
        };
        cost = cost.max(extra.unwrap_or(0));
    }
    cost
}

fn admit(spent: &[(i64, u32)], now: i64, units: u32, budget: u32, session: &str) -> Result<(), String> {
    let used: u32 = spent.iter().map(|(_, cost)| cost).sum();
    if used + units <= budget {
        return Ok(());
    }
    let frees = spent.iter().map(|(at, _)| at + WINDOW_SECONDS - now).min().map_or_else(String::new, |seconds| format!("; the oldest spend frees in {} min", (seconds.max(0) + 59) / 60));
    Err(format!("session {session} has spent {used} of its {budget} GitHub API units this hour and this call costs {units}{frees} (NORTH_GH_HOURLY_BUDGET sets the share)"))
}

fn fresh_spend(ledger: &Path, now: i64) -> Vec<(i64, u32)> {
    let text = fs::read_to_string(ledger).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (at, cost) = line.split_once(' ')?;
            Some((at.parse::<i64>().ok()?, cost.parse::<u32>().ok()?))
        })
        .filter(|(at, _)| now - at < WINDOW_SECONDS)
        .collect()
}

fn record(ledger: &Path, now: i64, units: u32) {
    let mut kept = fresh_spend(ledger, now);
    kept.push((now, units));
    let body: String = kept.iter().map(|(at, cost)| format!("{at} {cost}\n")).collect();
    let written = ledger.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| {
        let temporary = ledger.with_extension(format!("{}.tmp", std::process::id()));
        fs::File::create(&temporary)?.write_all(body.as_bytes())?;
        fs::rename(&temporary, ledger)
    });
    if let Err(error) = written {
        eprintln!("north gh-session: cannot record spend in {}: {error}", ledger.display());
    }
}

#[cfg(test)]
#[path = "gh_session_tests.rs"]
mod tests;
