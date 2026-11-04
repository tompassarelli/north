mod forecast;
mod live;
#[cfg(test)]
mod tests;
mod zone;

use std::sync::LazyLock;

use serde::Serialize;
use time::{Duration, PrimitiveDateTime, Time};
use toml_edit::DocumentMut;

pub use forecast::{Forecast, sleep_block};
pub use live::{live_signals, typed_messages};
pub use zone::{Moment, Zone, iso, parse};

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

fn registry(name: &str) -> i64 {
    DEFAULTS[name]["value"].as_integer().unwrap_or_else(|| panic!("harness/defaults.toml has no integer for {name}"))
}

pub static SLOT_MIN: LazyLock<i64> = LazyLock::new(|| registry("presence_slot_min"));
pub static WINDOW_DAYS: LazyLock<i64> = LazyLock::new(|| registry("presence_window_days"));
pub static PREDEPARTURE_LEAD_MIN: LazyLock<i64> = LazyLock::new(|| registry("predeparture_lead_min"));
pub static IDLE_REOPEN_SEC: LazyLock<i64> = LazyLock::new(|| registry("laptop_idle_reopen_sec"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Label {
    Exact,
    Heuristic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub at: Moment,
    pub label: Label,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IdleRow {
    pub at: Moment,
    pub idle: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct Away {
    pub from: Moment,
    pub to: Moment,
    pub kind: String,
    pub said: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub at: Option<Moment>,
    pub active: Option<bool>,
    pub session_start: Option<Moment>,
    pub idle_since: Option<Moment>,
}

#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub turns: Vec<Turn>,
    pub idle: Vec<IdleRow>,
    pub away: Vec<Away>,
    pub timezone: Option<Zone>,
    pub desktop: Option<Machine>,
    pub laptop: Option<Machine>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Evidence {
    Tag(&'static str),
    Stated { stated: String, kind: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Next {
    pub state: Option<&'static str>,
    pub at: Option<String>,
    pub p: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Presence {
    pub state: &'static str,
    pub pool_state: &'static str,
    pub since: String,
    pub next: Next,
    pub evidence: Vec<Evidence>,
}

struct Model<'a> {
    zone: &'a Zone,
    block: Option<(f64, f64)>,
    forecast: Forecast<'a>,
    away: &'a [Away],
}

impl Model<'_> {
    fn stated(&self, at: Moment) -> Option<&Away> {
        self.away.iter().find(|row| row.from <= at && at < row.to)
    }

    fn predicted(&self, at: Moment) -> (&'static str, f64) {
        if self.stated(at).is_some() {
            return ("travelling", 1.0);
        }
        let p = self.forecast.probability(at);
        if sleeping(at, self.block, self.zone) {
            return ("asleep", 1.0 - p);
        }
        if p >= 0.5 { ("present", p) } else { ("away", 1.0 - p) }
    }

    fn transition(&self, now: Moment, state: &str) -> Next {
        let step = Duration::minutes(*SLOT_MIN);
        let floor = now.replace_second(0).and_then(|at| at.replace_nanosecond(0)).unwrap_or(now);
        let mut slot = floor + Duration::minutes(*SLOT_MIN - i64::from(now.minute()) % *SLOT_MIN);
        for _ in 0..7 * 24 * 60 / *SLOT_MIN {
            let (candidate, p) = self.predicted(slot);
            if candidate != state && self.predicted(slot + step).0 == candidate {
                return Next { state: Some(candidate), at: Some(iso(slot)), p: Some(round4(p)) };
            }
            slot += step;
        }
        Next { state: None, at: None, p: None }
    }
}

fn round4(p: f64) -> f64 {
    format!("{p:.4}").parse().unwrap_or(p)
}

pub fn sleeping(at: Moment, block: Option<(f64, f64)>, zone: &Zone) -> bool {
    let Some((start, end)) = block else { return false };
    let local = zone.localize(at);
    let minute = f64::from(u16::from(local.hour()) * 60 + u16::from(local.minute()));
    if start < end { start <= minute && minute < end } else { minute >= start || minute < end }
}

struct Live {
    evidence: Vec<Evidence>,
    live: Vec<Moment>,
    idle_machines: usize,
}

fn machines(now: Moment, signals: &Signals) -> Live {
    let mut found = Live { evidence: Vec::new(), live: Vec::new(), idle_machines: 0 };
    for (name, row) in [("desktop", &signals.desktop), ("laptop", &signals.laptop)] {
        let Some(row) = row else { continue };
        let Some(at) = row.at.filter(|at| (Duration::ZERO..=Duration::seconds(120)).contains(&(now - *at))) else { continue };
        match row.active {
            Some(true) => {
                found.live.push(at);
                found.evidence.push(Evidence::Tag(name));
            }
            Some(false) => found.idle_machines += 1,
            None => {}
        }
    }
    found
}

fn idle_since(now: Moment, signals: &Signals, idle: &[&IdleRow]) -> Option<Moment> {
    let session_start = signals.desktop.as_ref().and_then(|desktop| desktop.session_start).unwrap_or(now);
    let mut current: Vec<&&IdleRow> = idle.iter().filter(|row| row.at >= session_start).collect();
    current.sort_by_key(|row| row.at);
    let mut since = None;
    for row in current {
        if row.idle == Some(true) {
            since = since.or(Some(row.at));
        } else {
            since = None;
        }
    }
    since
}

fn asleep_since(now: Moment, block: (f64, f64), zone: &Zone) -> Moment {
    let local = zone.localize(now);
    let clock = Time::from_hms((block.0 / 60.0).floor() as u8, (block.0 % 60.0) as u8, 0).unwrap_or(Time::MIDNIGHT);
    let wall = PrimitiveDateTime::new(local.date(), clock);
    let since = zone.resolve(wall);
    if since > now { zone.resolve(wall - Duration::days(1)) } else { since }
}

pub fn presence(now: Moment, signals: &Signals) -> Presence {
    let zone = signals.timezone.clone().unwrap_or(Zone::Fixed(now.offset().whole_seconds()));
    let turns: Vec<Moment> = signals.turns.iter().map(|row| row.at).filter(|at| *at <= now).collect();
    let idle: Vec<&IdleRow> = signals.idle.iter().filter(|row| row.at <= now).collect();
    let block = sleep_block(&turns, now, &zone);
    let model = Model { zone: &zone, block, forecast: Forecast::new(now, &turns, &idle, &zone), away: &signals.away };
    let Live { mut evidence, mut live, idle_machines } = machines(now, signals);
    let mut idle_since = idle_since(now, signals, &idle);
    let session = signals.desktop.as_ref().is_some_and(|desktop| desktop.session_start.is_some());
    let validated_idle = idle_machines == 2 && idle_since.is_some() && session;
    if let Some(latest) = turns.iter().filter(|at| now - Duration::minutes(*SLOT_MIN) <= **at).max() {
        live.push(*latest);
        evidence.push(Evidence::Tag("human-turn"));
    }
    let (state, since) = if let Some(first) = live.iter().min() {
        ("present", *first)
    } else if let Some(entry) = model.stated(now) {
        evidence = vec![Evidence::Stated { stated: entry.said.clone(), kind: entry.kind.clone() }];
        ("travelling", entry.from)
    } else if let Some(block) = block.filter(|_| sleeping(now, block, &zone)) {
        evidence.push(Evidence::Tag("learned-sleep"));
        ("asleep", asleep_since(now, block, &zone))
    } else if validated_idle {
        evidence.push(Evidence::Tag("live-idle"));
        ("away", idle_since.unwrap_or(now))
    } else {
        evidence.push(Evidence::Tag("incomplete-live-presence"));
        ("unknown", now)
    };
    let next = model.transition(now, state);
    if let (Some(since), Some(latest)) = (idle_since, turns.iter().max()) {
        idle_since = Some(since.max(*latest));
    }
    if let (Some(since), Some(laptop)) = (idle_since, signals.laptop.as_ref().and_then(|laptop| laptop.idle_since)) {
        idle_since = Some(since.max(laptop));
    }
    let sustained = validated_idle && idle_since.is_some_and(|since| (now - since).whole_seconds() >= *IDLE_REOPEN_SEC);
    let pool_state = if state == "present" || sustained { state } else { "unknown" };
    Presence { state, pool_state, since: iso(since), next, evidence }
}

const USAGE: &str = "usage: agents presence [--json] [--at ISO]";

pub fn run(arguments: &[String]) -> u8 {
    let mut json = false;
    let mut at = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--json" => json = true,
            "--at" => match rest.next().and_then(|text| parse(text)) {
                Some(moment) => at = Some(moment),
                None => return usage("--at wants an ISO time with a timezone"),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            other => return usage(&format!("unrecognized argument {other}")),
        }
    }
    let now = at.unwrap_or_else(|| {
        let utc = time::OffsetDateTime::now_utc();
        Zone::local(utc).localize(utc)
    });
    let result = presence(now, &live_signals(&|name| std::env::var(name).ok(), now, std::path::Path::new("/proc")));
    if json {
        println!("{}", serde_json::to_string(&result).expect("presence serializes"));
    } else {
        let next = &result.next;
        let p = next.p.map_or("None".to_owned(), |p| serde_json::to_string(&p).unwrap_or_default());
        println!(
            "{} since {}; next {} at {} (p={p})",
            result.state,
            result.since,
            next.state.unwrap_or("None"),
            next.at.as_deref().unwrap_or("None")
        );
    }
    0
}

fn usage(problem: &str) -> u8 {
    eprintln!("{USAGE}\nagents presence: {problem}");
    2
}
