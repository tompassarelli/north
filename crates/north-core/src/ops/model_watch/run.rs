use std::fs;
use std::time::Duration;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use super::host::Host;
use super::models::{Event, Source, SCHEMA, persist, proposal, release_candidates};
use super::{Options, digest, rank, sources};

fn sources_of(config: &Value, list: &str) -> Vec<Source> {
    let text = |item: &Value, field: &str| item[field].as_str().unwrap_or_default().to_owned();
    config[list].as_array().into_iter().flatten().map(|item| Source { name: text(item, "name"), parser: text(item, "parser"), endpoint: text(item, "endpoint") }).collect()
}

fn event_json(event: &Event, action: Option<&str>) -> Value {
    let mut value = json!({
        "model": event.row.model, "kind": event.row.kind, "title": event.row.title, "at": event.row.at, "p": event.row.p,
        "url": event.row.url, "basis": event.row.basis, "change": event.change, "ts": event.ts, "source": event.source, "id": event.id,
    });
    if let Some(action) = action {
        value["action"] = json!(action);
    }
    value
}

fn read_source(host: &dyn Host, db: &Connection, source: &Source, timeout: f64, threshold: f64) -> Result<(Vec<Event>, Value), String> {
    let payload = host.fetch(source, timeout)?;
    let rows = sources::normalize(source, &payload)?;
    let fetched = host.now().iso();
    let events = persist(db, &source.name, &rows, &fetched, threshold)?;
    let added = digest::observe(db, &source.name, &release_candidates(source, &payload, &rows)?, &fetched)?;
    let summary = json!({"source": source.name, "records": rows.len(), "events": events.len(), "candidates": added});
    Ok((events, summary))
}

fn record_proposals(db: &Connection, options: &Options, events: &[Event]) -> Result<Vec<Value>, String> {
    let sql = |error: rusqlite::Error| error.to_string();
    fs::create_dir_all(&options.proposals).map_err(|error| error.to_string())?;
    let mut shown = Vec::new();
    db.execute_batch("BEGIN").map_err(sql)?;
    let outcome = (|| -> Result<(), String> {
        for event in events {
            let (_, row) = proposal(event);
            let path = options.proposals.join(format!("{}.toml", event.id));
            fs::write(&path, row).map_err(|error| error.to_string())?;
            let mut detail = json!({"title": event.row.title, "url": event.row.url, "basis": event.row.basis, "change": event.change, "proposal": path.to_string_lossy(), "digest_status": "pending"});
            let action = (event.row.kind == "released").then_some("new model: run policy-replay.py --sweep against it");
            if let Some(action) = action {
                detail["action"] = json!(action);
            }
            db.execute("UPDATE model_events SET detail=? WHERE rowid=?", params![detail.to_string(), event.id]).map_err(sql)?;
            let mut candidate = detail.clone();
            for (field, value) in [("model", json!(event.row.model)), ("kind", json!(event.row.kind)), ("at", json!(event.row.at)), ("p", json!(event.row.p)), ("ts", json!(event.ts))] {
                candidate[field] = value;
            }
            db.execute("INSERT OR IGNORE INTO watch_candidates(id,source,title,url,detail) VALUES(?,?,?,?,?)", params![format!("m{}", event.id), event.source, event.row.title, event.row.url, candidate.to_string()]).map_err(sql)?;
            shown.push(event_json(event, action));
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => db.execute_batch("COMMIT").map_err(sql)?,
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(shown)
}

fn digest_source(host: &dyn Host, db: &Connection, source: &Source, timeout: f64) -> Result<Value, String> {
    let rows = digest::feed_rows(&source.parser, &host.fetch(source, timeout)?)?;
    let added = digest::observe(db, &source.name, &rows, &host.now().iso())?;
    Ok(json!({"source": source.name, "records": rows.len(), "events": added}))
}

pub(super) fn watch(host: &dyn Host, options: &Options, config: &Value) -> u8 {
    if let Some(parent) = options.db.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(db) = Connection::open(&options.db) else {
        eprintln!("north model-watch: ledger unavailable");
        return 1;
    };
    let _ = db.busy_timeout(Duration::from_secs(30));
    if let Err(error) = db.execute_batch(&format!("{SCHEMA}{}", digest::SCHEMA)) {
        eprintln!("north model-watch: {error}");
        return 1;
    }
    let threshold = config["market_move_threshold"].as_f64().unwrap_or(0.10);
    let (mut events, mut summaries) = (Vec::new(), Vec::new());
    for source in sources_of(config, "sources") {
        match read_source(host, &db, &source, options.timeout, threshold) {
            Ok((found, summary)) => {
                events.extend(found);
                summaries.push(summary);
            }
            Err(error) => summaries.push(json!({"source": source.name, "error": error})),
        }
    }
    let shown = match record_proposals(&db, options, &events) {
        Ok(shown) => shown,
        Err(error) => {
            eprintln!("north model-watch: proposals: {error}");
            return 1;
        }
    };
    for source in sources_of(config, "digest_sources") {
        summaries.push(digest_source(host, &db, &source, options.timeout).unwrap_or_else(|error| json!({"source": source.name, "error": error})));
    }
    let ranked = rank::ranked_digest(host, options, &db);
    let failed = ranked.get("error").is_some() || summaries.iter().any(|summary| summary.get("error").is_some());
    println!("{}", json!({"events": shown, "sources": summaries, "digest": ranked}));
    u8::from(failed)
}
