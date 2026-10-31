use rusqlite::Connection;

use super::row::Row;
use crate::threads::calibration::quantile;

type Series = Vec<Vec<f64>>;

fn capacity_series(db: &Connection) -> rusqlite::Result<Series> {
    let mut select = db.prepare(
        "SELECT family, repo, package, class, peakMemoryMiB FROM capacity_runs WHERE signature_exact = 1 AND peakMemoryMiB > 0 ORDER BY at",
    )?;
    let mut keys: Vec<[Option<String>; 4]> = Vec::new();
    let mut series: Series = Vec::new();
    let mut rows = select.query([])?;
    while let Some(row) = rows.next()? {
        let key = [row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?];
        let sample: f64 = row.get(4)?;
        match keys.iter().position(|known| *known == key) {
            Some(index) => series[index].push(sample),
            None => {
                keys.push(key);
                series.push(vec![sample]);
            }
        }
    }
    Ok(series)
}

fn charge(xs: &[f64]) -> f64 {
    xs.iter().copied().filter(|x| xs.iter().filter(|y| *y <= x).count() * 10 >= xs.len() * 9).fold(f64::INFINITY, f64::min)
}

pub(super) fn sweep_capacity_sizing(db: &Connection, margin: f64) -> rusqlite::Result<Vec<Row>> {
    let series = capacity_series(db)?;
    let mut out = Vec::new();
    for n in [5usize, 10, 20, 40, 1000] {
        let (mut under, mut slack, mut k) = (0usize, 0.0, 0usize);
        for xs in &series {
            for i in 3..xs.len() {
                let prior = &xs[i.saturating_sub(n)..i];
                let charged = charge(prior) * margin;
                k += 1;
                under += usize::from(xs[i] > charged);
                slack += charged / xs[i];
            }
        }
        out.push(Row::new().with("value", n).with("n", k).with("under_charged", under).with("mean_charged_over_peak", (k > 0).then(|| slack / k as f64)));
    }
    Ok(out)
}

pub(super) fn sweep_capacity_confidence(db: &Connection, margin: f64, window: usize) -> rusqlite::Result<Vec<Row>> {
    let bands = [(3usize, 5usize), (5, 10), (10, 20), (20, usize::MAX)];
    let mut ratios: Vec<Vec<f64>> = vec![Vec::new(); bands.len()];
    for xs in capacity_series(db)? {
        for i in 3..xs.len() {
            let prior = &xs[i.saturating_sub(window)..i];
            let band = bands.iter().position(|(lo, hi)| (*lo..*hi).contains(&i)).expect("bands cover every index from 3");
            ratios[band].push(xs[i] / (charge(prior) * margin));
        }
    }
    Ok(bands
        .iter()
        .zip(ratios)
        .map(|((lo, hi), mut rs)| {
            rs.sort_by(f64::total_cmp);
            let label = if *hi < usize::MAX { format!("{lo}-{}", hi - 1) } else { format!("{lo}+") };
            Row::new()
                .with("prior_runs", label)
                .with("n", rs.len())
                .with("over_charge", rs.iter().filter(|r| **r > 1.0).count())
                .with("p50_peak_over_charge", quantile(&rs, 0.5))
                .with("p90", quantile(&rs, 0.9))
                .with("max", rs.last().copied())
        })
        .collect())
}
