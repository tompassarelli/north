use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::Deserialize;

use super::run;
use crate::snapshot::Fleet;

pub const LOCAL: &str = "whiterabbit";

#[derive(Deserialize)]
struct Session {
    name: String,
    status: String,
    machine: Option<String>,
}

fn sessions() -> Result<Vec<Session>, String> {
    let text = run("north", &["fleet", "list", "--json"], Duration::from_secs(5))?;
    serde_json::from_str(&text).map_err(|error| format!("fleet list --json: {error}"))
}

pub fn live() -> Result<Fleet, String> {
    let mut servers: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for session in sessions()?.into_iter().filter(|session| session.status != "exited") {
        let server = session.machine.unwrap_or_else(|| LOCAL.into());
        *servers.entry(server).or_default().entry(session.status).or_default() += 1;
    }
    Ok(Fleet { source: "north fleet list".into(), servers })
}

pub fn machines() -> HashMap<String, String> {
    sessions().unwrap_or_default().into_iter().filter_map(|session| Some((session.name, session.machine?))).collect()
}

pub struct Run {
    pub session: String,
    pub started: u64,
    pub ended: u64,
}

pub fn runs(state: &std::path::Path, from: u64, to: u64) -> Result<Vec<Run>, String> {
    let ledger = state.join("ledger.db");
    let query = format!("select session, started_ms / 1000, ended_ms / 1000 from runs where ended_ms >= {} and started_ms <= {}", from * 1000, to * 1000);
    let text = run("sqlite3", &["-readonly", "-separator", "\t", &ledger.to_string_lossy(), &query], Duration::from_secs(30))?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some(Run { session: fields.next()?.to_owned(), started: fields.next()?.parse().ok()?, ended: fields.next()?.parse().ok()? })
        })
        .collect())
}

pub fn as_of(runs: &[Run], machines: &HashMap<String, String>, at: u64) -> Fleet {
    let mut state: BTreeMap<&str, &str> = BTreeMap::new();
    for run in runs {
        if run.started <= at && run.ended > at {
            state.insert(&run.session, "working");
        } else if run.ended <= at && run.ended + 1800 > at {
            state.entry(&run.session).or_insert("idle");
        }
    }
    let mut servers: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for (session, status) in state {
        let server = machines.get(session).cloned().unwrap_or_else(|| LOCAL.into());
        *servers.entry(server).or_default().entry(status.into()).or_default() += 1;
    }
    Fleet { source: "ledger runs".into(), servers }
}
