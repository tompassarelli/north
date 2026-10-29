use std::collections::BTreeMap;

use rusqlite::Connection;

use super::row::Row;
use crate::threads::calibration::quantile;
use crate::foundation::clock::Timestamp;

fn stamp(text: &str) -> f64 {
    Timestamp::parse_python_datetime(text).map_or(f64::NAN, Timestamp::unix_seconds_f64)
}

type Readout = (f64, f64, Option<String>);

pub(super) fn sweep_money_ceiling(db: &Connection, horizon: f64) -> rusqlite::Result<Vec<Row>> {
    let mut select = db.prepare(
        "SELECT provider, account, name, ts, used_pct, resets_at FROM usage WHERE kind = 'window' AND used_pct IS NOT NULL ORDER BY provider, account, name, ts",
    )?;
    let mut by: BTreeMap<(String, String, String), Vec<Readout>> = BTreeMap::new();
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let key = (row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get::<_, Option<String>>(1)?.unwrap_or_default(), row.get::<_, Option<String>>(2)?.unwrap_or_default());
        by.entry(key).or_default().push((stamp(&row.get::<_, String>(3)?), row.get(4)?, row.get(5)?));
    }
    let mut rises: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for ((provider, _, name), xs) in &by {
        for (i, (at, used, reset)) in xs.iter().enumerate() {
            let later = xs[i + 1..].iter().take_while(|(then, _, _)| then - at <= horizon * 60.0).filter(|(_, _, resets)| resets == reset).map(|(_, value, _)| *value);
            if let Some(peak) = later.reduce(f64::max) {
                let cell = format!("{provider} {}", name.rsplit('/').next().unwrap_or_default());
                rises.entry(cell).or_default().push((peak - used).max(0.0));
            }
        }
    }
    let all: Vec<f64> = rises.values().flatten().copied().collect();
    let max = |xs: &[f64]| xs.iter().copied().reduce(f64::max);
    let mut out: Vec<Row> = rises.iter().map(|(cell, xs)| Row::new().with("cell", cell.as_str()).with("n", xs.len()).with("p99", quantile(xs, 0.99)).with("max", max(xs))).collect();
    let p99 = quantile(&all, 0.99);
    out.push(
        Row::new()
            .with("cell", "all")
            .with("n", all.len())
            .with("p99", p99)
            .with("p999", quantile(&all, 0.999))
            .with("max", max(&all))
            .with("ceiling_p99", p99.map(|rise| 100.0 - rise)),
    );
    for ceiling in [85, 88, 90, 92, 95] {
        let overshoot = all.iter().filter(|rise| **rise > f64::from(100 - ceiling)).count();
        out.push(Row::new().with("value", ceiling).with("overshoot", overshoot).with("share", (!all.is_empty()).then(|| overshoot as f64 / all.len() as f64)));
    }
    Ok(out)
}
