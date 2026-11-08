//! `north dash`: one screen of the fleet over a window, failing things first: the readiness bars, each Claude
//! account's week and the spend by model and effort, and per task category which model and effort took it, how
//! often the first try landed and what a closed box cost. Every figure comes from runs or landings inside the
//! window; `--json` carries the same figures for machines, and the scoreboard's own reports sit behind its flags.
use std::path::PathBuf;

use serde_json::{Value, json};

use super::scoreboard::{self, ready, workers};
use crate::foundation::clock::Timestamp;
use crate::ledger::{self, ClosedReport, Ledger, WindowRun};
use crate::usage::outlook::{AccountOutlook, Runway};

mod billing;
mod dispatch;
mod render;

const USAGE: &str = "usage: north dash [--since 24h] [--json] | --closed --since 7d | --bars [--all] [--explain] | --weekly | --record | --frontier | --rework [--since 7d] | audit [--tell]";
const DEFAULT_WINDOW: &str = "24h";

/// What the screen reads, each section on its own so one unread source leaves the others standing.
pub struct Screen {
    pub window: String,
    pub now: i64,
    pub bars: Result<Vec<ready::Line>, String>,
    pub accounts: Result<Vec<AccountOutlook>, String>,
    pub runs: Result<(Vec<WindowRun>, ClosedReport), String>,
    pub attempts: Result<Vec<workers::Attempt>, String>,
    pub billing: Result<billing::Billing, String>,
}

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["audit", ..] | ["--weekly"] | ["--record"] | ["--frontier"] | ["--rework", ..] => return scoreboard::run(arguments),
        ["--bars", rest @ ..] => return scoreboard::run(&rest.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()),
        ["--closed", "--since", window] | ["--since", window, "--closed"] => return closed(window),
        _ => {}
    }
    let (mut window, mut json) = (DEFAULT_WINDOW, false);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match (*word, rest.clone().next()) {
            ("--json", _) => json = true,
            ("--since", Some(value)) => {
                window = value;
                rest.next();
            }
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    match read(window) {
        Ok(screen) if json => {
            println!("{}", figures(&screen));
            0
        }
        Ok(screen) => {
            print!("{}", render::screen(&screen));
            0
        }
        Err(why) => {
            eprintln!("north dash: {why}");
            1
        }
    }
}

/// The closed-box report dispatch and `north tune` decide on, as JSON: every closed box priced over all its runs.
fn closed(window: &str) -> u8 {
    let report = ledger::closed_report(window).and_then(|report| {
        let (linked, landed) = Ledger::open_default()?.landing_links(ledger::window_start_ms(window)?)?;
        Ok((report, linked, landed))
    });
    match report {
        Ok((report, linked, landed)) => {
            let mut figures = report.json(window);
            figures["landed_tasks"] = json!({"landed": landed, "linked": linked});
            println!("{figures}");
            0
        }
        Err(error) => {
            eprintln!("north dash: {}", error.user_message());
            1
        }
    }
}

fn read(window: &str) -> Result<Screen, String> {
    let now = Timestamp::now().unix_seconds();
    let since = ledger::window_start_ms(window).map_err(|error| error.user_message())? / 1_000;
    let span = now - since;
    let (bars, accounts, runs, attempts) = std::thread::scope(|scope| {
        let bars = scope.spawn(|| ready::lines(now, span));
        let accounts = scope.spawn(accounts);
        let runs = scope.spawn(|| ledger::inside(window).map_err(|error| error.user_message()));
        let attempts = scope.spawn(|| first_tries(since, now));
        let joined = |name: &str| format!("{name} reader panicked");
        (bars.join().unwrap_or_else(|_| Err(joined("bars"))), accounts.join().unwrap_or_else(|_| Err(joined("usage"))), runs.join().unwrap_or_else(|_| Err(joined("runs"))), attempts.join().unwrap_or_else(|_| Err(joined("first tries"))))
    });
    Ok(Screen { window: window.to_owned(), now, bars, accounts, runs, attempts, billing: billing::read() })
}

fn accounts() -> Result<Vec<AccountOutlook>, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no home directory")?;
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot)?;
    crate::usage::outlook::claude_accounts(&db, &crate::platform::orchestration::path(), Timestamp::now())
}

/// The worker first tries that started inside the window, judged as the first-try bar judges them.
fn first_tries(since: i64, now: i64) -> Result<Vec<workers::Attempt>, String> {
    let path = ledger::threads_database();
    let db = scoreboard::open(&path)?;
    let missed = workers::missed_checks(&north_github_mirror::store::Mirror::local());
    let attempts = workers::attempts(&db, since, &missed, &workers::Pending::read(&db, now)).map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(attempts.into_iter().filter(|attempt| attempt.started >= since && !attempt.deferred).collect())
}

fn runway(runway: Runway) -> Value {
    match runway {
        Runway::Out => json!("out"),
        Runway::Empties { minutes } => json!({"empties_in_min": minutes.round()}),
        Runway::Lasts => json!("lasts"),
        Runway::Unknown => Value::Null,
    }
}

fn figures(screen: &Screen) -> Value {
    let section = |result: Result<Value, &String>| result.unwrap_or_else(|why| json!({"unread": why}));
    let accounts = screen.accounts.as_ref().map(|accounts| {
        accounts.iter().map(|account| json!({
            "account": account.account,
            "week_left_pct": account.week_used.map(|used| (100.0 - used).max(0.0)),
            "burn_pct_per_h": account.burn_per_hour,
            "runway": runway(account.runway),
            "week_resets": account.week_resets.map(Timestamp::iso),
            "plan": screen.billing.as_ref().ok().and_then(|billing| billing.plans.get(&account.account)),
        })).collect::<Vec<_>>().into()
    });
    let mixes = dispatch::Mixes::of(screen);
    json!({
        "since": screen.window,
        "at": Timestamp::from_unix_seconds(screen.now).iso(),
        "bars": section(screen.bars.as_ref().map(|bars| json!(bars))),
        "accounts": section(accounts),
        "spend": section(mixes.as_ref().map(|mixes| json!(mixes.spend))),
        "spend_by_model": section(mixes.as_ref().map(|mixes| json!(mixes.models))),
        "infra": section(screen.billing.as_ref().map(|billing| json!({"machines": billing.machines, "monthly_usd": billing.total_usd(), "capped": billing.capped()}))),
        "dispatch": section(mixes.as_ref().map(|mixes| json!(mixes.rows))),
    })
}

#[cfg(test)]
mod tests;
