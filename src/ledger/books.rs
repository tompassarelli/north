use std::collections::BTreeMap;

use rusqlite::Row;

use crate::error::NorthResult;
use crate::fleet::books::{Books, Landing, SessionSpend};

use super::pricing::{PriceTable, TokenUse};
use super::{Ledger, millis};

impl Books for Ledger {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        let prices = PriceTable::load()?;
        let rows = self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT session, model, sum(tokens_in), sum(tokens_out), sum(tokens_cache_read), sum(tokens_cache_write) FROM runs GROUP BY session, model ORDER BY session",
            )?;
            statement.query_map([], spend_row)?.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let mut sessions: BTreeMap<String, SessionSpend> = BTreeMap::new();
        for (session, model, tokens) in rows {
            let price = model.as_ref().and_then(|model| prices.models.get(model));
            let entry = sessions.entry(session.clone()).or_insert_with(|| SessionSpend { session, tokens: 0, api_usd: Some(0.0) });
            entry.tokens += [tokens.input, tokens.output, tokens.cache_read, tokens.cache_write].into_iter().map(|count| u64::try_from(count).unwrap_or(0)).sum::<u64>();
            entry.api_usd = entry.api_usd.zip(price.map(|price| price.of(tokens))).map(|(sum, cost)| sum + cost);
        }
        Ok(sessions.into_values().collect())
    }

    fn landed_since(&self, since_ms: u64) -> NorthResult<Vec<Landing>> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT task_ref, landed_sha, wall_ms, landed_ms FROM tasks WHERE outcome = 'landed' AND landed_ms >= ?1 ORDER BY landed_ms DESC",
            )?;
            statement.query_map([millis(since_ms)], landing_row)?.collect()
        })
    }
}

fn spend_row(row: &Row<'_>) -> rusqlite::Result<(String, Option<String>, TokenUse)> {
    let tokens = TokenUse { input: row.get(2)?, output: row.get(3)?, cache_read: row.get(4)?, cache_write: row.get(5)? };
    Ok((row.get(0)?, row.get(1)?, tokens))
}

fn landing_row(row: &Row<'_>) -> rusqlite::Result<Landing> {
    let wall_ms: Option<i64> = row.get(2)?;
    let landed_ms: i64 = row.get(3)?;
    Ok(Landing {
        task_ref: row.get(0)?,
        sha: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        wall_ms: wall_ms.and_then(|wall| u64::try_from(wall).ok()),
        landed_ms: u64::try_from(landed_ms).unwrap_or(0),
    })
}
