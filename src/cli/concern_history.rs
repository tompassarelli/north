use north_goals::history::{Recorded, Sample, Store};
use north_goals::trend::Point;
use serde_json::Value;

use crate::foundation::clock::Timestamp;

pub(crate) struct LedgerHistory;

impl Store for LedgerHistory {
    fn record(&self, samples: &[Sample]) -> Result<(), String> {
        let mut batch = String::new();
        for sample in samples {
            let mut row = serde_json::to_value(sample).map_err(|error| error.to_string())?;
            row["type"] = Value::from(crate::threads::concerns::KIND);
            batch.push_str(&format!("{row}\n"));
        }
        if batch.is_empty() { Ok(()) } else { crate::threads::client::ingest(batch.as_bytes()) }
    }

    fn read(&self, host: &str, since: Timestamp) -> Result<Vec<Recorded>, String> {
        let rows = crate::threads::concerns::since(&crate::ledger::threads_database(), host, since)?;
        Ok(rows.into_iter().map(|row| Recorded { project: row.project, concern: row.concern, point: Point { at: row.at.unix_seconds(), value: row.value } }).collect())
    }
}
