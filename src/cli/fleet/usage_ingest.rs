use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use crate::fleet::runtime::now_ms;
use crate::fleet::usage_sink::UsageSink;
use crate::usage::events;

#[derive(Default)]
pub(in crate::cli) struct IngestUsage {
    last: Mutex<HashMap<String, Vec<Value>>>,
}

impl IngestUsage {
    fn batch(&self, provider: &str, account: &str, frame: &Value, at: f64) -> Option<String> {
        let rows = if provider == "claude" { events::stream_rows(frame, at) } else { Vec::new() };
        if rows.is_empty() || !self.changed(account, &rows) {
            return None;
        }
        Some(
            rows.into_iter()
                .map(|mut row| {
                    row["account"] = Value::from(account);
                    row["type"] = Value::from("usage");
                    format!("{row}\n")
                })
                .collect(),
        )
    }

    fn changed(&self, account: &str, rows: &[Value]) -> bool {
        let shape: Vec<Value> = rows.iter().map(|row| Value::Array(["kind", "name", "used_pct", "resets_at", "source"].iter().map(|key| row[*key].clone()).collect())).collect();
        let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        last.insert(account.to_owned(), shape.clone()) != Some(shape)
    }
}

impl UsageSink for IngestUsage {
    fn observe(&self, provider: &str, account: &str, frame: &Value) {
        let Some(batch) = self.batch(provider, account, frame, now_ms() as f64 / 1000.0) else { return };
        std::thread::spawn(move || {
            if let Err(error) = crate::threads::client::ingest(batch.as_bytes()) {
                eprintln!("north fleet: stream usage rows not recorded: {error}");
            }
        });
    }
}
