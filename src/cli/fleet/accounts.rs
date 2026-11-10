use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

pub(super) fn run(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    match flags {
        [] => listing(fleet, false),
        ["--claude-ai"] => listing(fleet, true),
        ["--api"] => api_pool_listing(),
        ["--plan"] => super::router::account_plan(fleet),
        ["--reading", account, usd] => reading(account, usd),
        ["--probe"] => probe(),
        [name] => super::router::session_verb(fleet, "accounts", name),
        _ => Err(super::usage::usage()),
    }
}

/// Records the balance Tom reads in the provider's console, so it overrides north's list-price estimate until an Admin API reading exists; `north fleet accounts --plan` then names the figure and when it was read.
fn reading(account: &str, usd: &str) -> NorthResult<()> {
    let usd: f64 = usd.trim_start_matches('$').parse().map_err(|_| NorthError::Usage(format!("{usd} is not a dollar amount")))?;
    let (home, _) = places();
    let state = home.join(".local/state");
    let line = north_core::credit::readings::record_console(&policy(), &state, account, usd, Timestamp::now().unix_seconds()).map_err(NorthError::State)?;
    println!("{line}; the reading is kept in {}", state.join("north/api-readings").display());
    Ok(())
}

/// Re-judges every held exhausted mark with one one-token probe per organization, clearing each that passes.
fn probe() -> NorthResult<()> {
    let (home, state) = places();
    for line in crate::providers::claude::api_pool::rejudge(&policy(), &home, &state, &crate::platform::host::name(), crate::providers::claude::api_probe::probe) {
        println!("{line}");
    }
    Ok(())
}

fn policy() -> std::path::PathBuf {
    crate::platform::orchestration::path()
}

fn places() -> (std::path::PathBuf, std::path::PathBuf) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    (home.clone(), home.join(".local/state/north"))
}

fn api_pool_listing() -> NorthResult<()> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let policy = crate::platform::orchestration::path();
    for line in crate::providers::claude::api_pool::listing(&policy, &home, &home.join(".local/state/north"), &crate::platform::host::name()) {
        println!("{line}");
    }
    Ok(())
}

fn listing(fleet: &Fleet, claude_ai: bool) -> NorthResult<()> {
    let roster = fleet.roster();
    for account in roster.accounts.iter().filter(|account| !claude_ai || account.home.join(".credentials.json").is_file()) {
        match &account.api {
            Some(key) => println!("{}\t{}\t{}", account.provider.label(), account.label, key.org),
            None => println!("{}\t{}", account.provider.label(), account.label),
        }
    }
    for omitted in roster.omitted() {
        eprintln!("omitted: {omitted}");
    }
    Ok(())
}
