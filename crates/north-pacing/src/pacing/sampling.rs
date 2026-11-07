use super::{process, readout, storage};
use crate::foundation::clock::rfc3339_seconds;
use crate::spend::{self, Placed, Rules, Size, Starter, Turn};
use crate::platform::harness;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use storage::{load, number, rows, save};

pub(super) fn config() -> Result<toml_edit::DocumentMut, String> {
    std::fs::read_to_string(crate::platform::orchestration::path())
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())
}
/// The worker turn the ledger prices on each Claude account at `at`: the p90 API-equivalent dollars of a worker run over the last
/// `TURN_DAYS` days at the account's measured points per dollar. An account the ledger cannot price has none.
pub(super) fn worker_turns(at: f64) -> impl Fn(&str) -> Option<Turn> {
    const TURN_DAYS: i64 = 7;
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = rusqlite::Connection::open_with_flags(crate::ledger::threads_database(), flags).ok();
    let since = crate::foundation::clock::Timestamp::from_unix_seconds(at as i64 - TURN_DAYS * 86_400).iso();
    let cost = db.as_ref().and_then(|db| crate::threads::calibration::TurnCost::read(db, &since).ok().flatten());
    let now = crate::foundation::clock::Timestamp::from_unix_seconds(at as i64);
    move |account| cost?.on(db.as_ref()?, account.strip_prefix("claude:")?, now).ok().flatten()
}

pub(super) type Reads<'a> = BTreeMap<&'a str, (Vec<Value>, Value)>;

/// The sessions taking a turn on each account and the worker sessions taking a turn across every registry, as the spend cap counts
/// them; dispatch reads them from every registry.
pub struct Workers {
    pub by_account: BTreeMap<String, i64>,
    pub active: usize,
}

/// Reads `Workers` from every registry the orchestration config names, or none when one does not answer.
pub type FleetWorkers = fn(&toml_edit::DocumentMut, &str, f64) -> Option<Workers>;
static FLEET_WORKERS: std::sync::OnceLock<FleetWorkers> = std::sync::OnceLock::new();

pub(super) fn install_fleet_workers(read: FleetWorkers) {
    let _ = FLEET_WORKERS.set(read);
}

const ADAPTERS: [&str; 2] = ["claude", "gemini"];

fn due_adapters<'a>(prior: &Value, provider: Option<&str>, at: f64) -> Vec<&'a str> {
    ADAPTERS
        .iter()
        .copied()
        .filter(|p| provider.is_none_or(|only| only == *p) && at >= number(&prior["providers"][*p]["next_at"], 0.0))
        .collect()
}

type Invocation<'a> = (&'a str, bool, Vec<String>);

pub(super) fn adapter_commands<'a>(due: &[&'a str], owners: &[String], local: &[&str], refresh: bool, program: &str) -> Vec<Invocation<'a>> {
    due.iter()
        .flat_map(|&p| {
            let quiet = (!refresh && p == "claude").then_some("--no-refresh");
            let argv = move |prefix: &[&str]| -> Vec<String> { prefix.iter().copied().chain(["fleet", "usage", "--rows", p]).chain(quiet).map(str::to_owned).collect() };
            let remotes = owners.iter().filter(move |_| p == "claude").map(move |destination| (p, true, argv(&["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north"])));
            let own = local.contains(&p).then(|| (p, false, argv(&[program])));
            own.into_iter().chain(remotes.collect::<Vec<_>>())
        })
        .collect()
}

fn read_adapters<'a>(due: &[&'a str], owners: &[String], local: &[&str], refresh: bool) -> Reads<'a> {
    let commands = adapter_commands(due, owners, local, refresh, &process::binary());
    std::thread::scope(|scope| {
        let jobs: Vec<_> = commands
            .iter()
            .map(|(p, remote, argv)| {
                let job = scope.spawn(move || {
                    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
                    process::adapter(&argv[0], &args)
                });
                (*p, *remote, job)
            })
            .collect();
        let mut reads = Reads::new();
        for (p, remote, job) in jobs {
            let (rows, status) = job.join().unwrap_or_else(|_| (Vec::new(), json!({"code":1,"elapsed_s":0,"throttled":false})));
            let read = reads.entry(p).or_insert_with(|| (Vec::new(), Value::Null));
            read.0.extend(rows);
            if !remote || read.1.is_null() {
                read.1 = status;
            }
        }
        reads
    })
}

/// Reads usage rows for the Claude accounts this host holds a token for; the provider layer supplies it.
pub type TokenUsage = fn(&std::path::Path, f64) -> Vec<Value>;
static TOKEN_USAGE: std::sync::OnceLock<TokenUsage> = std::sync::OnceLock::new();

pub(super) fn install_token_usage(read: TokenUsage) {
    let _ = TOKEN_USAGE.set(read);
}

fn token_rows(at: f64) -> Vec<Value> {
    TOKEN_USAGE.get().map_or_else(Vec::new, |read| read(&crate::platform::orchestration::path(), at))
}

pub(super) fn merge_token_rows(reads: &mut Reads, rows: impl FnOnce() -> Vec<Value>) {
    if let Some(claude) = reads.get_mut("claude") {
        claude.0.extend(rows());
    }
}

pub(super) fn read_here(cfg: &toml_edit::DocumentMut, provider: &str, here: &str) -> bool {
    let Some(accounts) = cfg.get("accounts").and_then(|accounts| accounts.get(provider)) else {
        return false;
    };
    accounts.get("profiles").and_then(toml_edit::Item::as_table_like).is_none_or(|profiles| profiles.iter().any(|(_, hosts)| hosts.as_table_like().is_some_and(|hosts| hosts.contains_key(here))))
}

pub(super) fn tokens_cover_claude(cfg: &toml_edit::DocumentMut, here: &str) -> bool {
    let profiles = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")).and_then(toml_edit::Item::as_table_like);
    profiles.is_some_and(|table| !table.is_empty() && table.iter().all(|(_, hosts)| hosts.get(here).and_then(|entry| entry.get("token_file")).is_some()))
}

pub(super) fn claude_owners(cfg: &toml_edit::DocumentMut, here: &str) -> Vec<String> {
    let profiles = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")).and_then(toml_edit::Item::as_table_like);
    let mut hosts: Vec<&str> = profiles.into_iter().flat_map(|table| table.iter()).filter_map(|(_, hosts)| hosts.as_table_like()).flat_map(|hosts| hosts.iter().map(|(host, _)| host)).filter(|host| *host != here).collect();
    hosts.sort_unstable();
    hosts.dedup();
    hosts.into_iter().filter_map(|host| cfg.get("hosts")?.get(host)?.get("ssh")?.as_str().map(str::to_owned)).collect()
}

fn ingest_remote(directory: &std::path::Path, fresh: &[Value], at: f64) -> Result<(), String> {
    let input = fresh
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row["type"] = json!("usage");
            format!("{row}\n")
        })
        .collect::<String>();
    let result = process::run(
        &super::process::binary(),
        &["threads", "ingest"],
        Some(&input),
        10.0,
    );
    let code = result.map_or(127, |r| r.code);
    if code != 0 {
        storage::append(
            &directory.join("pace-control.jsonl"),
            &json!({"at":at,"ingest_code":code,"retained":"pace-control.json history"}),
        )?;
    }
    Ok(())
}

pub(super) fn record_cadence(
    directory: &std::path::Path,
    state: &mut Value,
    reads: &Reads,
    at: f64,
) -> Result<(), String> {
    for (provider, (_, measured)) in reads {
        let previous = number(&state["providers"][*provider]["interval_s"], 60.0);
        let failed = measured["throttled"] == true || measured["code"] != 0;
        let cadence = interval(previous, number(&measured["elapsed_s"], 0.0), failed);
        let mut measured = measured.clone();
        measured["interval_s"] = json!(cadence);
        measured["sampled_at"] = if failed { state["providers"][*provider]["sampled_at"].clone() } else { json!(at) };
        measured["next_at"] = json!(at + cadence);
        state["providers"][*provider] = measured.clone();
        measured["provider"] = json!(provider);
        measured["at"] = json!(at);
        storage::append(&directory.join("pace-control.jsonl"), &measured)?;
    }
    Ok(())
}

const MIN_INTERVAL_SECONDS: f64 = 60.0;
const SLOW_READ_SECONDS: f64 = 20.0;

pub(super) fn interval(previous: f64, elapsed: f64, throttled: bool) -> f64 {
    if throttled {
        return MIN_INTERVAL_SECONDS.max(previous * 2.0).min(crate::usage::STALE_SECONDS);
    }
    if elapsed > SLOW_READ_SECONDS { MIN_INTERVAL_SECONDS.max((elapsed * 2.0).ceil()) } else { MIN_INTERVAL_SECONDS }
}

fn usage_gate(db: &rusqlite::Connection) -> Result<Value, String> {
    crate::usage::write_pace_gate(db).map_err(|error| format!("usage gate failed: {error}"))
}

pub(super) fn prune_retired(state: &mut Value) {
    let live = |provider: &str| ADAPTERS.contains(&provider);
    if let Some(providers) = state["providers"].as_object_mut() {
        providers.retain(|name, _| live(name));
    }
    if let Some(history) = state["history"].as_array_mut() {
        history.retain(|entry| entry["provider"].as_str().is_none_or(live));
    }
}

fn measure(directory: &std::path::Path, db: &rusqlite::Connection, reads: &Reads, at: f64) -> Result<(Value, Value), String> {
    let _lock = storage::lock(directory, "pace-control.lock", None)?;
    let loaded = load(&directory.join("pace-control.json"));
    let mut state = json!({"providers": loaded["providers"], "history": loaded["history"]});
    if !state["providers"].is_object() {
        state["providers"] = json!({});
    }
    prune_retired(&mut state);
    record_cadence(directory, &mut state, reads, at)?;
    readout::record(db, rows(&state["history"]))?;
    Ok((state, usage_gate(db)?))
}

pub(super) fn history_window(remote: bool) -> f64 {
    if remote { 10800.0 } else { 600.0 }
}

pub(super) fn recent_history(state: &Value, fresh: &[Value], window: f64) -> Vec<Value> {
    rows(&state["history"])
        .iter()
        .chain(fresh.iter())
        .filter(|r| r["ts"].as_str().and_then(rfc3339_seconds).is_some_and(|ts| ts >= storage::now() - window))
        .cloned()
        .collect()
}

pub(super) fn standings(directory: &std::path::Path, cfg: &toml_edit::DocumentMut, workers: &Workers, at: f64) -> Vec<Value> {
    let rules = Rules::configured();
    let catalog = harness::document(harness::MODEL_CATALOG);
    let turn = worker_turns(at);
    let view = spend::view(directory, &rules, workers.active, at, |name| Size { plan_week: spend::plan_week(cfg, catalog, name), turn: turn(name) }, |_| true, spend::keyed(cfg));
    view.accounts
        .iter()
        .map(|account| {
            let refusal = view.admit(&rules, &account.name, Starter::Worker, Placed::default()).err().map(|refusal| refusal.to_string());
            json!({"at": at, "account": account.name, "admits": refusal.is_none(), "refusal": refusal, "workers": workers.by_account.get(&account.name).copied().unwrap_or(0)})
        })
        .collect()
}

pub(super) fn sample(provider: Option<&str>) -> Result<Value, String> {
    let directory = storage::directory();
    let _sampling = storage::lock(&directory, "pace-sampling.lock", None)?;
    let at = storage::now();
    let prior = load(&directory.join("pace-control.json"));
    let here = crate::platform::host::name();
    let cfg = config().ok();
    let owners = cfg.as_ref().map(|cfg| claude_owners(cfg, &here)).unwrap_or_default();
    let local: Vec<&str> = ADAPTERS.iter().copied().filter(|p| cfg.as_ref().is_none_or(|cfg| read_here(cfg, p, &here))).collect();
    let refresh = !cfg.as_ref().is_some_and(|cfg| tokens_cover_claude(cfg, &here));
    let mut reads = read_adapters(&due_adapters(&prior, provider, at), &owners, &local, refresh);
    if local.contains(&"claude") {
        merge_token_rows(&mut reads, || token_rows(at));
    }
    let fresh: Vec<_> = reads.values().flat_map(|(rows, _)| rows.clone()).collect();
    let (db, remote) = readout::open()?;
    readout::record(&db, &fresh)?;
    if remote && !fresh.is_empty() {
        ingest_remote(&directory, &fresh, at)?;
    }
    let cfg = config()?;
    let (mut state, gate) = measure(&directory, &db, &reads, at)?;
    let workers = FLEET_WORKERS.get().and_then(|read| read(&cfg, &here, at));
    let _lock = storage::lock(&directory, "pace-control.lock", None)?;
    for standing in workers.as_ref().map(|workers| standings(&directory, &cfg, workers, storage::now())).unwrap_or_default() {
        storage::append(&directory.join("pace-control.jsonl"), &standing)?;
    }
    state["history"] = json!(recent_history(&state, &fresh, history_window(remote)));
    save(&directory.join("pace-control.json"), &state)?;
    Ok(gate)
}
