use std::collections::BTreeMap;
use std::fmt;

use crate::foundation::clock::parse_day_number;

use super::log::Read;

#[derive(Debug, PartialEq, Eq)]
pub enum Key {
    Missing,
    NoExpiry,
    Unreadable(String),
    Expires { date: String, days: i64 },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Audit {
    pub peer: String,
    pub calls: usize,
    pub refusals: usize,
    pub failures: usize,
    pub unreadable: usize,
    pub last: Option<String>,
    pub refused: BTreeMap<String, usize>,
    pub key: Key,
}

pub fn audit(peer: &str, log: &Read, authorized_keys: Option<&str>, today: i64) -> Audit {
    let rows: Vec<_> = log.rows.iter().filter(|row| row.peer == peer).collect();
    let mut refused = BTreeMap::new();
    for reason in rows.iter().filter_map(|row| row.refusal.as_deref()) {
        *refused.entry(reason.to_string()).or_insert(0) += 1;
    }
    Audit {
        peer: peer.to_string(),
        calls: rows.len(),
        refusals: rows.iter().filter(|row| row.refusal.is_some()).count(),
        failures: rows.iter().filter(|row| row.refusal.is_none() && row.exit != 0).count(),
        unreadable: log.unreadable,
        last: rows.iter().map(|row| row.ts.clone()).max(),
        refused,
        key: authorized_keys.map_or(Key::Missing, |text| key(text, peer, today)),
    }
}

pub fn key(authorized_keys: &str, peer: &str, today: i64) -> Key {
    let expiries: Vec<Option<&str>> = authorized_keys.lines().filter(|line| serves(line, peer)).map(|line| option(line, "expiry-time")).collect();
    if expiries.is_empty() {
        return Key::Missing;
    }
    let mut dated = Vec::new();
    for expiry in expiries {
        let Some(expiry) = expiry else { return Key::NoExpiry };
        let date = expiry.get(..8).filter(|digits| digits.bytes().all(|b| b.is_ascii_digit())).map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]));
        match date.as_deref().and_then(parse_day_number).zip(date) {
            Some((day, date)) => dated.push((day, date)),
            None => return Key::Unreadable(expiry.to_string()),
        }
    }
    dated.into_iter().min().map_or(Key::Missing, |(day, date)| Key::Expires { date, days: day - today })
}

fn serves(line: &str, peer: &str) -> bool {
    option(line.trim_start(), "command").is_some_and(|command| {
        let words: Vec<&str> = command.split_whitespace().collect();
        words.contains(&"remote-serve") && words.windows(2).any(|pair| pair == ["--peer", peer])
    })
}

fn option<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = line;
    loop {
        let (field, after) = field(rest)?;
        if let Some(value) = field.strip_prefix(name).and_then(|value| value.strip_prefix('=')) {
            return Some(value.trim_matches('"'));
        }
        rest = after.strip_prefix(',')?;
    }
}

fn field(text: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (index, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' | ' ' | '\t' if !quoted => return Some((&text[..index], &text[index..])),
            _ => {}
        }
    }
    (!text.is_empty()).then_some((text, ""))
}

impl fmt::Display for Audit {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(out, "peer {}", self.peer)?;
        writeln!(out, "calls {}", self.calls)?;
        writeln!(out, "refusals {}", self.refusals)?;
        writeln!(out, "failures {}", self.failures)?;
        if self.unreadable > 0 {
            writeln!(out, "unreadable rows {}", self.unreadable)?;
        }
        if let Some(last) = &self.last {
            writeln!(out, "last call {last}")?;
        }
        for (reason, count) in &self.refused {
            writeln!(out, "refused {count}: {reason}")?;
        }
        match &self.key {
            Key::Missing => writeln!(out, "key not found in authorized_keys"),
            Key::NoExpiry => writeln!(out, "key has no expiry-time"),
            Key::Unreadable(value) => writeln!(out, "key expiry-time {value} is unreadable"),
            Key::Expires { date, days } => writeln!(out, "key expires {date} in {days} days"),
        }
    }
}
