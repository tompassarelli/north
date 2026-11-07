use crate::error::NorthResult;
use crate::fleet::registry::Fleet;

pub(super) fn run(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    match flags {
        [] => listing(fleet, false),
        ["--claude-ai"] => listing(fleet, true),
        ["--api"] => api_pool_listing(),
        ["--plan"] => super::router::account_plan(fleet),
        [name] => super::router::session_verb(fleet, "accounts", name),
        _ => Err(super::usage::usage()),
    }
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
