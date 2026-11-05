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

const USAGE: &str = "usage: north proxy [--interactive] | north proxy calls TRANSCRIPT | north proxy move [--to ACCOUNT] [--auto] [--dry-run]";
const OPENING: &str = "[north: north proxy started this session as Tom's proxy]";

pub(in crate::cli) fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["move", rest @ ..] => window_move::run(rest),
        [] => router::open_fleet().and_then(attach),
        ["--interactive"] => router::open_fleet().and_then(|fleet| interactive::run(&fleet, PROXY)),
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
    tui::fleet::run_on(fleet, PROXY)
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
    let main = main.to_string_lossy();
    router::launch(fleet, &["--provider", "claude", "--role", "proxy", "--parent", "none", "--cwd", &main, "--take-over", &opening])
}

fn home() -> NorthResult<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))
}
