use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::profile::Trace;
use super::{AFTER, COVERED_BY, Category, PHASE, Position, Span, Spans};

const SETTLE_S: f64 = 3.0 * 3600.0;
const MARGIN_S: f64 = 60.0;
const KEEP_S: f64 = 8.0 * 86_400.0;
const KEPT: [&str; 3] = [PHASE, COVERED_BY, "landed"];

/// A landing whose after-push spans are all in: its seconds are kept, and `traces` are the traces a later span would have to name to change them.
#[derive(Serialize, Deserialize)]
struct Settled {
    id: String,
    end: f64,
    wall: f64,
    traces: Vec<String>,
}

/// What an earlier read of one spans file left: where it stopped, the settled landings from `since` on, and the recent spans the unsettled landings are still read from.
#[derive(Default, Serialize, Deserialize)]
struct State {
    since: f64,
    position: Option<Position>,
    settled: Vec<Settled>,
    recent: Vec<Span>,
}

impl State {
    fn changed_by(&self, spans: &[Span], now: f64) -> bool {
        let ids: HashSet<&str> = self.settled.iter().map(|landing| landing.id.as_str()).collect();
        let traces: HashSet<&str> = self.settled.iter().flat_map(|landing| landing.traces.iter().map(String::as_str)).collect();
        spans.iter().any(|span| ids.contains(span.id.as_str()) || traces.contains(span.trace.as_str()) || (span.category == Category::Landing && span.end < now - SETTLE_S))
    }
}

fn trimmed(span: &Span) -> Span {
    let attributes: BTreeMap<String, String> = KEPT.iter().filter_map(|key| Some(((*key).to_owned(), span.attribute(key)?.to_owned()))).collect();
    Span { trace: span.trace.clone(), id: span.id.clone(), parent: span.parent.clone(), name: String::new(), category: span.category, start: span.start, end: span.end, host: String::new(), cpu: None, attributes }
}

fn traces_of(by_trace: &HashMap<&str, Vec<&Span>>, landing: &Span) -> Vec<String> {
    let Some(landed) = landing.attribute("landed") else { return Vec::new() };
    let covering = by_trace.get(landed).into_iter().flatten().filter(|span| span.attribute(PHASE) == Some(AFTER)).filter_map(|span| span.attribute(COVERED_BY));
    std::iter::once(landed).chain(covering).map(str::to_owned).collect::<HashSet<String>>().into_iter().collect()
}

/// The end and after-push wall seconds of each landing in `spans` that ended at or after `since`, as `Trace::profile` reads them from the whole file, read from the bytes the file gained since `cache` was written.
///
/// A landing settles once it ended `SETTLE_S` ago and keeps its seconds; a span that then names one of its traces, or a
/// landing recorded that late, makes the read start over from the whole file.
pub fn after_walls(spans: &Spans, cache: &Path, since: f64, now: f64) -> Vec<(i64, f64)> {
    let floor = since.min(now - KEEP_S);
    let mut state: State = fs::read(cache).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).filter(|state: &State| state.since <= floor).unwrap_or_default();
    let mut tail = spans.read_tail(state.position.as_ref());
    let rebuilt = tail.restarted || state.changed_by(&tail.spans, now);
    if rebuilt {
        if !tail.restarted {
            tail = spans.read_tail(None);
        }
        state = State::default();
    }
    let advanced = rebuilt || !tail.spans.is_empty();
    state.position = tail.position;
    state.recent.extend(tail.spans.iter().map(trimmed));
    let cutoff = now - SETTLE_S;
    let (live, newly) = {
        let trace = Trace::of(&state.recent);
        let settled: HashSet<&str> = state.settled.iter().map(|landing| landing.id.as_str()).collect();
        let mut by_trace: HashMap<&str, Vec<&Span>> = HashMap::new();
        for span in &state.recent {
            by_trace.entry(span.trace.as_str()).or_default().push(span);
        }
        let (mut live, mut newly) = (Vec::new(), Vec::new());
        for landing in trace.landings(floor).into_iter().filter(|landing| !settled.contains(landing.id.as_str())) {
            let wall = trace.profile(landing).after_wall;
            if landing.end < cutoff {
                newly.push(Settled { id: landing.id.clone(), end: landing.end, wall, traces: traces_of(&by_trace, landing) });
            } else {
                live.push((landing.end, wall));
            }
        }
        (live, newly)
    };
    let settled_now = !newly.is_empty();
    state.settled.extend(newly);
    state.settled.retain(|landing| landing.end >= floor);
    state.recent.retain(|span| span.end >= cutoff - MARGIN_S);
    state.since = floor;
    if advanced || settled_now {
        save(cache, &state);
    }
    let mut walls: Vec<(f64, f64)> = state.settled.iter().filter(|landing| landing.end >= since).map(|landing| (landing.end, landing.wall)).chain(live.into_iter().filter(|(end, _)| *end >= since)).collect();
    walls.sort_by(|left, right| left.0.total_cmp(&right.0));
    walls.into_iter().filter(|(_, wall)| *wall > 0.0).map(|(end, wall)| (end as i64, wall)).collect()
}

fn save(path: &Path, state: &State) {
    let written = path.parent().and_then(|directory| fs::create_dir_all(directory).ok().and_then(|()| tempfile::NamedTempFile::new_in(directory).ok())).and_then(|file| {
        serde_json::to_writer(std::io::BufWriter::new(file.as_file()), state).ok()?;
        file.persist(path).ok()
    });
    if written.is_none() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "verified_tests.rs"]
mod tests;
