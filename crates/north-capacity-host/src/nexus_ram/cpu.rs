use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct CpuStat {
    pub(super) total_jiffies: u64,
    pub(super) steal_jiffies: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(super) struct Pressure {
    pub(super) some_avg10: f64,
    pub(super) full_avg10: Option<f64>,
    pub(super) some_total_us: u64,
    pub(super) full_total_us: Option<u64>,
    pub(super) some_delta_us: Option<u64>,
    pub(super) full_delta_us: Option<u64>,
}

#[derive(Debug, PartialEq, Serialize)]
pub(super) struct CpuRow {
    pub(super) gap_s: Option<i64>,
    pub(super) steal_pct: Option<f64>,
    pub(super) cpu_stat: Option<CpuStat>,
    pub(super) cpu_psi: Option<Pressure>,
    pub(super) io_psi: Option<Pressure>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Previous {
    ts: String,
    #[serde(default)]
    cpu_stat: Option<CpuStat>,
    #[serde(default)]
    cpu_psi: Option<Pressure>,
    #[serde(default)]
    io_psi: Option<Pressure>,
}

pub(super) fn psi(text: &str) -> HashMap<String, HashMap<String, String>> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let kind = fields.next()?.to_owned();
            let values = fields.filter_map(|field| field.split_once('=')).map(|(key, value)| (key.to_owned(), value.to_owned()));
            Some((kind, values.collect()))
        })
        .collect()
}

pub(super) fn pressure(text: &str) -> Option<Pressure> {
    let lines = psi(text);
    let field = |kind: &str, key: &str| lines.get(kind)?.get(key)?.parse::<f64>().ok();
    let total = |kind: &str| lines.get(kind)?.get("total")?.parse::<u64>().ok();
    Some(Pressure {
        some_avg10: field("some", "avg10")?,
        full_avg10: field("full", "avg10"),
        some_total_us: total("some")?,
        full_total_us: total("full"),
        some_delta_us: None,
        full_delta_us: None,
    })
}

pub(super) fn cpu_stat(text: &str) -> Option<CpuStat> {
    let line = text.lines().find(|line| line.split_whitespace().next() == Some("cpu"))?;
    let jiffies = line.split_whitespace().skip(1).take(8).map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok()?;
    Some(CpuStat { total_jiffies: jiffies.iter().sum(), steal_jiffies: *jiffies.get(7)? })
}

fn since(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
    current?.checked_sub(previous?)
}

fn steal_pct(current: CpuStat, previous: CpuStat) -> Option<f64> {
    let total = current.total_jiffies.checked_sub(previous.total_jiffies).filter(|total| *total > 0)?;
    let steal = current.steal_jiffies.checked_sub(previous.steal_jiffies)?;
    Some((steal as f64 * 10_000.0 / total as f64).round() / 100.0)
}

fn pressure_since(current: &mut Pressure, previous: Pressure) {
    current.some_delta_us = since(Some(current.some_total_us), Some(previous.some_total_us));
    current.full_delta_us = since(current.full_total_us, previous.full_total_us);
}

pub(super) fn apply_previous(row: &mut CpuRow, ts: &str, previous: &Previous) {
    let Ok(then) = Timestamp::parse(&previous.ts) else {
        return;
    };
    row.gap_s = Timestamp::parse(ts).ok().map(|now| now.unix_seconds() - then.unix_seconds());
    row.steal_pct = row.cpu_stat.zip(previous.cpu_stat).and_then(|(current, before)| steal_pct(current, before));
    for (current, before) in [(&mut row.cpu_psi, previous.cpu_psi), (&mut row.io_psi, previous.io_psi)] {
        if let (Some(current), Some(before)) = (current.as_mut(), before) {
            pressure_since(current, before);
        }
    }
}
