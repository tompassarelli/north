//! `north dash`: the board over a window. Its first line is landings and closed boxes per hour, then one panel per
//! concern in the operator views' order: Work, Accounts, Infra and Health, each opening on its headline fact. Every
//! panel is a projection of this snapshot; `--json` carries the same figures for machines, and the scoreboard's own
//! reports sit behind its flags.
use std::path::PathBuf;

use serde_json::{Value, json};

use super::scoreboard::{self, ready, workers};
use crate::fleet::board::Board;
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;
use crate::ledger::{self, ClosedReport, Ledger, WindowRun};
use crate::usage::outlook::{AccountOutlook, Runway};

pub(crate) use crate::cli::infra::manifest as billing;
mod board;
mod dispatch;
mod escalations;
mod habits;
mod render;
mod spend;

const USAGE: &str = "usage: north dash [--since 24h] [--json] | --closed --since 7d | --bars [--all] [--explain] | --weekly | --record | --frontier | --rework [--since 7d] | audit [--tell] | losses [--since 24h] [--until DATE] [--repo north]";
const DEFAULT_WINDOW: &str = "24h";

/// What the screen reads, each section on its own so one unread source leaves the others standing.
pub struct Screen {
    pub window: String,
    pub now: i64,
    /// The window's start in unix seconds, so the screen and its figures read one clock.
    pub since: i64,
    pub bars: Result<Vec<ready::Line>, String>,
    pub accounts: Result<Vec<AccountOutlook>, String>,
    pub runs: Result<(Vec<WindowRun>, ClosedReport), String>,
    pub attempts: Result<Vec<workers::Attempt>, String>,
    /// The dispatch records inside the window, from which each escalation and lift check are read.
    pub launches: Vec<north_dispatch::dispatch::LaunchRecord>,
    pub habits: Result<Vec<habits::Row>, String>,
    /// The runs of the last 7 days, from which each subscription's week is sized in dollars.
    pub week_runs: Result<Vec<WindowRun>, String>,
    pub billing: Result<billing::Billing, String>,
    pub infra: Result<(Vec<crate::cli::infra::report::Resource>, Vec<String>), String>,
    /// Each api organization's credit, as `north fleet usage`, `north fleet accounts --plan` and the fleet screen print it.
    pub credit: Vec<north_core::credit::Standing>,
    /// The fleet store's sessions, gates, landings and held messages, the Work part of the snapshot.
    pub work: Result<board::Work, String>,
}

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["audit", ..] | ["losses", ..] | ["--weekly"] | ["--record"] | ["--frontier"] | ["--rework", ..] => return scoreboard::run(arguments),
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
            print!("{}", render::screen(&screen, colored()));
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
    let fleet = crate::cli::fleet::open_fleet().ok();
    read_with_fleet(window, fleet.as_ref())
}

pub(crate) fn read_with_fleet(window: &str, fleet: Option<&Fleet>) -> Result<Screen, String> {
    let now = Timestamp::now().unix_seconds();
    let since = ledger::window_start_ms(window).map_err(|error| error.user_message())? / 1_000;
    let span = now - since;
    let since_ms = u64::try_from(since.saturating_mul(1_000)).unwrap_or(0);
    let (bars, accounts, runs, attempts, habits, week_runs, work, infra) = std::thread::scope(|scope| {
        let bars = scope.spawn(|| ready::lines(now, span));
        let accounts = scope.spawn(accounts);
        let runs = scope.spawn(|| ledger::inside(window).map_err(|error| error.user_message()));
        let attempts = scope.spawn(|| first_tries(since, now));
        let habits = scope.spawn(|| habits::rows(&ledger::threads_database(), &Timestamp::from_unix_seconds(since).iso(), &Timestamp::from_unix_seconds(now).iso()));
        let week_runs = scope.spawn(|| ledger::runs_inside("7d").map_err(|error| error.user_message()));
        let work = scope.spawn(|| match fleet {
            Some(fleet) => board::work(fleet, since_ms),
            None => Err("the fleet store is unreadable".to_owned()),
        });
        let infra = scope.spawn(|| {
            let read = crate::cli::infra::read_with_billing();
            (read.billing, read.resources, read.unread)
        });
        let joined = |name: &str| format!("{name} reader panicked");
        (bars.join().unwrap_or_else(|_| Err(joined("bars"))), accounts.join().unwrap_or_else(|_| Err(joined("usage"))), runs.join().unwrap_or_else(|_| Err(joined("runs"))), attempts.join().unwrap_or_else(|_| Err(joined("first tries"))), habits.join().unwrap_or_else(|_| Err(joined("habits"))), week_runs.join().unwrap_or_else(|_| Err(joined("week runs"))), work.join().unwrap_or_else(|_| Err(joined("work"))), infra.join().unwrap_or_else(|_| (Err(joined("infra")), Vec::new(), Vec::new())))
    });
    let (billing, resources, unread) = infra;
    Ok(Screen { window: window.to_owned(), now, since, bars, accounts, runs, attempts, launches: north_dispatch::dispatch::launch_records(&north_dispatch::dispatch::launch_directory()), habits, week_runs, billing, infra: Ok((resources, unread)), credit: credit(), work })
}

/// The Board the fleet screen reads: the same snapshot `north dash` prints, over the board's day window.
pub(crate) fn for_fleet(fleet: &Fleet) -> Board {
    match read_with_fleet(DEFAULT_WINDOW, Some(fleet)) {
        Ok(screen) => board::of(&screen),
        Err(why) => Board::unread(why),
    }
}

/// Colour only for a person at a terminal: never into a pipe or file, under NO_COLOR, or on a dumb terminal.
fn colored() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()) && std::env::var("TERM").is_ok_and(|term| term != "dumb")
}

fn credit() -> Vec<north_core::credit::Standing> {
    north_core::credit::standing(&crate::platform::orchestration::path(), &crate::watch::state_home(), Timestamp::now()).orgs
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
    Ok(attempts.into_iter().filter(|attempt| attempt.started >= since && attempt.started < now && !attempt.deferred).collect())
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
    let escalations = match &screen.runs {
        Ok((runs, _)) => json!(escalations::Escalations::of(&screen.launches, runs, screen.since as f64, screen.now as f64)),
        Err(why) => json!({"unread": why}),
    };
    let spent = spend::Spend::of(screen).map(|spent| json!({"classes": spent.classes.iter().map(|class| json!({"mix": class.mix, "runs": class.runs, "usd": class.usd, "week_pct": class.week_pct, "all_pct": class.all_pct})).collect::<Vec<_>>(), "api_credit": {"runs": spent.api.runs, "usd": spent.api.usd}}));
    let habits = screen.habits.as_ref().map(|rows| {
        let rewrites: usize = rows.iter().map(|row| row.rewrites).sum();
        let warnings: usize = rows.iter().map(|row| row.warnings).sum();
        json!({"rewrites": rewrites, "warnings": warnings, "rules": rows.iter().map(|row| json!({"rule": row.rule, "rewrites": row.rewrites, "warnings": row.warnings})).collect::<Vec<_>>()})
    });
    json!({
        "since": screen.window,
        "at": Timestamp::from_unix_seconds(screen.now).iso(),
        "bars": section(screen.bars.as_ref().map(|bars| json!(bars))),
        "accounts": section(accounts),
        "api_credit_left": screen.credit.iter().map(|org| json!({"account": format!("api:{}", org.name), "left_usd": org.left, "estimate_usd": org.estimated, "read_by": org.reading.as_ref().map(|reading| reading.source.label()), "exhausted": org.exhausted, "figure": org.figure()})).collect::<Vec<_>>(),
        "spend": section(mixes.as_ref().map(|mixes| json!(mixes.spend))),
        "subscription_spend": section(spent.as_ref().cloned()),
        "spend_by_model": section(mixes.as_ref().map(|mixes| json!(mixes.models))),
        "infra": match &screen.infra {
            Ok((resources, unread)) => json!({"resources": resources, "unread": unread, "total": crate::cli::infra::report::Report::of(Timestamp::from_unix_seconds(screen.now).iso(), resources.clone(), unread.clone()).total_line()}),
            Err(why) => json!({"unread": why}),
        },
        "work": match &screen.work {
            Ok(work) => json!({"sessions": work.sessions.iter().map(|session| json!({"name": session.name, "status": session.status, "task": session.task})).collect::<Vec<_>>(), "gates": work.gates.len(), "landed": work.landed.len(), "held": work.held}),
            Err(why) => json!({"unread": why}),
        },
        "dispatch": section(mixes.as_ref().map(|mixes| json!(mixes.rows))),
        "cells": section(mixes.as_ref().map(|mixes| json!(mixes.cells))),
        "escalations": escalations,
        "habits": section(habits),
    })
}

#[cfg(test)]
mod tests;
