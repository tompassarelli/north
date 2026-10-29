use super::{
    admission,
    control::{self, Control, Reading},
    process, readout, storage, target,
    window::Window,
};
use crate::platform::harness;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use storage::{load, number, rows, save};

pub(super) fn root() -> PathBuf {
    harness::harness_root().unwrap_or_else(|skew| {
        eprintln!("{skew}");
        std::process::exit(1)
    })
}
pub(super) fn policy_path() -> PathBuf {
    std::env::var_os("AGENTS_ORCHESTRATION")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("AGENTS_OPERATOR_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join("code/south/main/agents")
                })
                .join("orchestration.toml")
        })
}
pub(super) fn config() -> Result<toml_edit::DocumentMut, String> {
    std::fs::read_to_string(policy_path())
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())
}
pub(super) type Reads<'a> = BTreeMap<&'a str, (Vec<Value>, Value)>;

const ADAPTERS: [(&str, &str); 2] = [("claude", "claude"), ("gemini", "agy")];

fn due_adapters<'a>(prior: &Value, provider: Option<&str>, at: f64) -> Vec<&'a (&'a str, &'a str)> {
    ADAPTERS
        .iter()
        .filter(|(p, _)| {
            provider.is_none_or(|only| only == *p)
                && at >= number(&prior["providers"][*p]["next_at"], 0.0)
        })
        .collect()
}

fn read_adapters<'a>(due: &[&'a (&'a str, &'a str)], owners: &[String], local: &[&str], refresh: bool) -> Reads<'a> {
    let commands: Vec<(&str, bool, Vec<String>)> = due
        .iter()
        .flat_map(|(p, a)| {
            let script = root().join(format!("harness/lib/usage_{a}.py")).to_string_lossy().into_owned();
            let remote = format!(".local/state/north/current/tree/harness/lib/usage_{a}.py");
            let quiet = (!refresh && *p == "claude").then_some("--no-refresh");
            let remotes = owners.iter().filter(|_| *p == "claude").map(move |destination| {
                (*p, true, ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "python3", "-I", &remote].into_iter().chain(quiet).map(str::to_owned).collect())
            });
            let own = local.contains(p).then(|| (*p, false, ["python3", "-I", &script].into_iter().chain(quiet).map(str::to_owned).collect()));
            own.into_iter().chain(remotes.collect::<Vec<_>>())
        })
        .collect();
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
pub(crate) type TokenUsage = fn(&std::path::Path, f64) -> Vec<Value>;
static TOKEN_USAGE: std::sync::OnceLock<TokenUsage> = std::sync::OnceLock::new();

pub(super) fn install_token_usage(read: TokenUsage) {
    let _ = TOKEN_USAGE.set(read);
}

fn token_rows(at: f64) -> Vec<Value> {
    TOKEN_USAGE.get().map_or_else(Vec::new, |read| read(&policy_path(), at))
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
        &root().join("bin/threads").to_string_lossy(),
        &["ingest"],
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

fn record_cadence(
    directory: &std::path::Path,
    state: &mut Value,
    reads: &Reads,
    at: f64,
) -> Result<(), String> {
    for (provider, (_, measured)) in reads {
        let previous = number(&state["providers"][*provider]["interval_s"], 60.0);
        let cadence = admission::interval(
            previous,
            number(&measured["elapsed_s"], 0.0),
            measured["throttled"] == true || measured["code"] != 0,
            0.0,
        );
        let mut measured = measured.clone();
        measured["interval_s"] = json!(cadence);
        measured["next_at"] = json!(at + cadence);
        state["providers"][*provider] = measured.clone();
        measured["provider"] = json!(provider);
        measured["at"] = json!(at);
        storage::append(&directory.join("pace-control.jsonl"), &measured)?;
    }
    Ok(())
}

fn usage_gate(db: &rusqlite::Connection) -> Result<Value, String> {
    crate::usage::write_pace_gate(db).map_err(|error| format!("usage gate failed: {error}"))
}

pub(super) fn prune_retired(state: &mut Value) {
    let live = |provider: &str| ADAPTERS.iter().any(|(name, _)| *name == provider);
    for key in ["accounts", "providers"] {
        if let Some(entries) = state[key].as_object_mut() {
            entries.retain(|name, _| live(name.split(':').next().unwrap_or_default()));
        }
    }
    for (key, field) in [("history", "provider"), ("grants", "account")] {
        if let Some(entries) = state[key].as_array_mut() {
            entries.retain(|entry| entry[field].as_str().is_none_or(|name| live(name.split(':').next().unwrap_or_default())));
        }
    }
}

fn measure(
    directory: &std::path::Path,
    db: &rusqlite::Connection,
    reads: &Reads,
    at: f64,
) -> Result<(Value, Value), String> {
    let _lock = storage::lock(directory, "pace-control.lock", None)?;
    let mut state = load(&directory.join("pace-control.json"));
    for key in ["accounts", "providers"] {
        if !state[key].is_object() {
            state[key] = json!({});
        }
    }
    prune_retired(&mut state);
    record_cadence(directory, &mut state, reads, at)?;
    readout::record(db, rows(&state["history"]))?;
    let gate = usage_gate(db)?;
    Ok((state, gate))
}

fn lead_is_live(grant: &Value, nodes: &[Value]) -> bool {
    nodes.iter().any(|n| {
        n["name"] == grant["lead"]
            && n["conversation"] == grant["session"]
            && n["status"] != "exited"
            && grant["account"].as_str().is_some_and(|a| {
                n["provider"] == a.split(':').next().unwrap_or_default()
                    && (!a.starts_with("claude:")
                        || n["account"] == a.trim_start_matches("claude:"))
            })
    })
}

pub(super) fn history_window(remote: bool) -> f64 {
    if remote { 10800.0 } else { 600.0 }
}

pub(super) fn recent_history(state: &Value, fresh: &[Value], window: f64) -> Vec<Value> {
    rows(&state["history"])
        .iter()
        .chain(fresh.iter())
        .filter(|r| {
            r["ts"]
                .as_str()
                .and_then(target::seconds)
                .is_some_and(|ts| ts >= storage::now() - window)
        })
        .cloned()
        .collect()
}

struct Round<'a> {
    directory: &'a std::path::Path,
    providers: Vec<&'a str>,
    nodes: Vec<Value>,
    fresh: Vec<Value>,
    remote: bool,
    initial: i64,
    notify: bool,
}

fn publish(
    round: &Round,
    state: &mut Value,
    gate: &mut Value,
) -> Result<Vec<(String, Control)>, String> {
    let directory = round.directory;
    let _lock = storage::lock(directory, "pace-control.lock", None)?;
    let retained = load(&directory.join("pace-control.json"));
    state["grants"] = json!(
        rows(&retained["grants"])
            .iter()
            .filter(|g| lead_is_live(g, &round.nodes))
            .collect::<Vec<_>>()
    );
    let mut deliveries = update_controls(
        state,
        gate,
        &round.providers,
        &round.nodes,
        storage::now(),
        round.initial,
        target::target_for,
    )?;
    for (account, control) in &deliveries {
        let mut raw = serde_json::to_value(control).map_err(|e| e.to_string())?;
        raw["account"] = json!(account);
        storage::append(&directory.join("pace-control.jsonl"), &raw)?;
    }
    deliveries.retain(|(_, c)| round.notify && c.notify);
    state["history"] = json!(recent_history(state, &round.fresh, history_window(round.remote)));
    save(&directory.join("pace-control.json"), state)?;
    save(&directory.join("usage-gate.json"), gate)?;
    Ok(deliveries)
}

fn deliver_all(deliveries: &[(String, Control)]) {
    std::thread::scope(|scope| {
        for (account, control) in deliveries {
            scope.spawn(move || {
                if let Err(error) = deliver(account, control) {
                    eprintln!("pace-control: delivery failed for {account}: {error}");
                }
            });
        }
    });
}

pub(super) fn sample(provider: Option<&str>, initial: i64, notify: bool) -> Result<Value, String> {
    let directory = storage::directory();
    let _sampling = storage::lock(&directory, "pace-sampling.lock", None)?;
    let at = storage::now();
    let prior = load(&directory.join("pace-control.json"));
    let here = crate::platform::host::name();
    let cfg = config().ok();
    let owners = cfg.as_ref().map(|cfg| claude_owners(cfg, &here)).unwrap_or_default();
    let local: Vec<&str> = ADAPTERS.iter().map(|(p, _)| *p).filter(|p| cfg.as_ref().is_none_or(|cfg| read_here(cfg, p, &here))).collect();
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
    config()?;
    let (mut state, mut gate) = measure(&directory, &db, &reads, at)?;
    let round = Round {
        directory: &directory,
        providers: reads.keys().copied().collect(),
        nodes: process::fleet(),
        fresh,
        remote,
        initial,
        notify,
    };
    let deliveries = publish(&round, &mut state, &mut gate)?;
    deliver_all(&deliveries);
    Ok(gate)
}

fn window_summary(w: &Window) -> String {
    format!(
        "{} used={:.1}% burn={}%/h horizon={:.2}h projected={}%",
        w.window,
        w.used_pct,
        w.burn_pct_h
            .map(|n| json!(n).to_string())
            .unwrap_or_else(|| "None".into()),
        w.horizon_h.unwrap_or(w.reset_h),
        w.projected_pct
            .map(|n| json!(n).to_string())
            .unwrap_or_else(|| "None".into())
    )
}

fn control_line(account: &str, control: &Control, action: &str) -> String {
    let mut numbers = control
        .windows
        .iter()
        .map(window_summary)
        .collect::<Vec<_>>()
        .join("; ");
    if let Some(target) = &control.target {
        numbers.push_str(&format!(
            " target={}% by={}",
            json!(target.spend_to_pct),
            target.by
        ));
    }
    format!(
        "{action} {account} allowed={} {numbers}",
        control.allowed_workers.unwrap_or(0)
    )
}

pub(super) fn phone_notice(account: &str, control: &Control) -> (String, String) {
    let action = if control.held == Some(true) {
        "PACE-HELD"
    } else {
        "PACE-RESUMED"
    };
    (
        format!("Pace: {account} {action}"),
        control_line(account, control, action),
    )
}

pub(super) fn deliver(account: &str, control: &Control) -> Result<(), String> {
    let (title, line) = phone_notice(account, control);
    let output = process::run(
        &root().join("bin/supply-alert").to_string_lossy(),
        &["normal", &line, &title],
        None,
        15.0,
    )?;
    if output.code != 0 {
        eprintln!("pace-control: phone notification failed for {account}");
    }
    Ok(())
}

pub(super) fn update_controls(
    state: &mut Value,
    gate: &mut Value,
    providers: &[&str],
    nodes: &[Value],
    at: f64,
    initial: i64,
    target_for: impl Fn(&str, f64) -> Result<Option<super::target::Target>, String>,
) -> Result<Vec<(String, Control)>, String> {
    if !state["accounts"].is_object() {
        state["accounts"] = json!({});
    }
    let mut deliveries = Vec::new();
    for provider in providers {
        let projections = gate[provider]["projections"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut controls = json!({});
        for (account, windows) in projections {
            let windows: Vec<Window> =
                serde_json::from_value(windows).map_err(|e| e.to_string())?;
            let previous: Control =
                serde_json::from_value(state["accounts"][&account].clone()).unwrap_or_default();
            let (p, label) = account.split_once(':').unwrap_or((&account, ""));
            let count = nodes
                .iter()
                .filter(|n| {
                    n["status"] == "working"
                        && n["provider"] == p
                        && (p != "claude" || n["account"] == label)
                })
                .count();
            let current = at;
            let control = control::advance(
                &previous,
                Reading {
                    windows,
                    workers: Some(count as i64),
                    at: current,
                    initial,
                    margin: 10.0,
                    target: target_for(&account, current)?,
                },
            );
            deliveries.push((account.clone(), control.clone()));
            let raw = serde_json::to_value(&control).map_err(|e| e.to_string())?;
            controls[&account] = raw.clone();
            state["accounts"][&account] = raw.clone();
            gate[provider]["projections"][&account] = json!(control.windows);
        }
        gate[provider]["controls"] = controls;
    }

    Ok(deliveries)
}
