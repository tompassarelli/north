//! The ceiling on one service's messages to the proxy, whatever their condition keys: past three in ten
//! minutes, the next is folded into one line carrying the count that goes out in its place and is
//! recorded as a `page-flood` incident, and the rest of the window waits in `north fleet digest`. On
//! 2026-11-10 one lander condition reached the proxy several hundred times in fifteen minutes because
//! nothing below the senders bounded the rate, so a sender with a wrong key or none cannot flood.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sender::Sender;

pub const FILE: &str = "page-rates.json";
pub const WINDOW_MS: u64 = 10 * 60 * 1000;
pub const LIMIT: usize = 3;
pub const CLASS: &str = "page-flood";

/// What the ceiling makes of a service's message to the proxy: it is under the ceiling, it is folded into the page-flood line carried here, or this window's flood was already told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ceiling {
    Under,
    Flood(String),
    Over,
}

#[derive(Default, Deserialize, Serialize)]
struct Rate {
    sent_ms: Vec<u64>,
    folded_ms: Option<u64>,
}

/// Whether `sender`'s next message to the proxy goes out, is folded into the page-flood line, or waits because this window's flood was already told.
pub fn admit(root: &Path, sender: &Sender, text: &str, now_ms: u64) -> Ceiling {
    let path = root.join(FILE);
    let mut rates: BTreeMap<String, Rate> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let within = |at: &u64| now_ms.saturating_sub(*at) < WINDOW_MS;
    for rate in rates.values_mut() {
        rate.sent_ms.retain(within);
        rate.folded_ms = rate.folded_ms.filter(within);
    }
    rates.retain(|_, rate| !rate.sent_ms.is_empty() || rate.folded_ms.is_some());
    let rate = rates.entry(sender.to_string()).or_default();
    let admitted = if rate.sent_ms.len() < LIMIT {
        rate.sent_ms.push(now_ms);
        Ceiling::Under
    } else if rate.folded_ms.is_some() {
        Ceiling::Over
    } else {
        rate.sent_ms.push(now_ms);
        rate.folded_ms = Some(now_ms);
        Ceiling::Flood(line(sender, rate.sent_ms.len(), text))
    };
    let scratch = path.with_extension("json.part");
    if let Ok(json) = serde_json::to_string(&rates)
        && fs::write(&scratch, json).is_ok()
    {
        let _ = fs::rename(&scratch, &path);
    }
    admitted
}

fn line(sender: &Sender, count: usize, text: &str) -> String {
    format!(
        "north fleet: page flood: {sender} sent {count} messages to the proxy within {} minutes, past the ceiling of {LIMIT}, so its next messages for {} minutes wait in `north fleet digest`, which counts them, and the flood is recorded as a {CLASS} incident; the folded one: {text}",
        WINDOW_MS / 60_000,
        WINDOW_MS / 60_000
    )
}
