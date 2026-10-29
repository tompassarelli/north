use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const FRESH_USAGE_MILLISECONDS: f64 = 3000.0;
const BATCH_CLASSES: [&str; 4] = ["moderate", "heavy", "exclusive", "gpu"];

/// A live run lease as the host stores it, with the usage sample its wrapper last wrote.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LeaseFact {
    pub(crate) id: String,
    pub(crate) class: String,
    pub(crate) owner: String,
    #[serde(default)]
    pub(crate) quiet: bool,
    #[serde(default)]
    pub(crate) pair: Option<u32>,
    pub(crate) created_at: f64,
    #[serde(default)]
    pub(crate) quiet_seconds: Option<f64>,
    #[serde(default)]
    pub(crate) usage: Option<UsageSample>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSample {
    #[serde(default)]
    at: Option<f64>,
    #[serde(default)]
    last_busy_at: Option<f64>,
}

/// A waiting ticket as the host reads it from the queue directory.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueEntry {
    name: String,
    class: String,
    #[serde(default)]
    alone: bool,
    #[serde(default)]
    queued_at: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuietNative {
    pub(crate) owner: String,
    pub(crate) held_seconds: f64,
    pub(crate) idle_seconds: Option<f64>,
    pub(crate) run_seconds: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PairLease {
    pub(crate) pair: u32,
    pub(crate) owner: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct PairRefusal {
    pub(crate) pair: u32,
    pub(crate) holder: String,
    #[serde(rename = "freePair")]
    pub(crate) free_pair: u32,
}

pub(crate) struct Derived {
    pub(crate) quiet_native: Vec<QuietNative>,
    pub(crate) pair_leases: Vec<PairLease>,
    pub(crate) alone_queued_ahead: u32,
}

impl LeaseFact {
    fn quiet_view(&self, now: f64) -> QuietNative {
        let measured = self.usage.as_ref().and_then(|usage| Some((usage.at?, usage.last_busy_at?))).filter(|(at, _)| now - at <= FRESH_USAGE_MILLISECONDS);
        QuietNative {
            owner: self.owner.clone(),
            held_seconds: (now - self.created_at) / 1000.0,
            idle_seconds: measured.map(|(_, last_busy_at)| (now - last_busy_at) / 1000.0),
            run_seconds: self.quiet_seconds,
        }
    }

    fn is_quiet_native(&self) -> bool {
        self.class == "native" && self.quiet
    }
}

impl QuietNative {
    /// A quiet lease holds batch work until its clients have measured idle for `idle_bound` seconds or it has run past its declared length; an unmeasured lease counts as busy.
    pub(crate) fn holds(&self, idle_bound: f64) -> bool {
        let past_run = self.run_seconds.is_some_and(|run| self.held_seconds > run);
        let idle = self.idle_seconds.is_some_and(|idle| idle >= idle_bound);
        !past_run && !idle
    }
}

pub(crate) fn holding_owners(leases: &[QuietNative], idle_bound: f64) -> Vec<String> {
    leases.iter().filter(|lease| lease.holds(idle_bound)).map(|lease| lease.owner.clone()).collect()
}

pub(crate) fn derive(leases: &[LeaseFact], queue: &[QueueEntry], ticket: Option<&str>, now: f64) -> Derived {
    Derived {
        quiet_native: leases.iter().filter(|lease| lease.is_quiet_native()).map(|lease| lease.quiet_view(now)).collect(),
        pair_leases: leases.iter().filter(|lease| lease.class == "native").filter_map(|lease| Some(PairLease { pair: lease.pair?, owner: lease.owner.clone() })).collect(),
        alone_queued_ahead: queue.iter().filter(|entry| entry.class == "native" && entry.alone && ticket.is_none_or(|ticket| entry.name.as_str() < ticket)).count().try_into().unwrap_or(u32::MAX),
    }
}

/// The holder of the pair a request names when another owner has it live, with the lowest pair number nobody holds.
pub(crate) fn pair_refusal(pair: Option<u32>, owner: Option<&str>, leases: &[PairLease]) -> Option<PairRefusal> {
    let pair = pair?;
    let holder = leases.iter().find(|lease| lease.pair == pair && Some(lease.owner.as_str()) != owner)?;
    let free_pair = (0..).find(|candidate| leases.iter().all(|lease| lease.pair != *candidate))?;
    Some(PairRefusal { pair, holder: holder.owner.clone(), free_pair })
}

/// A queued alone request, and every native lease behind one, waits until no Warcraft client is live.
pub(crate) fn alone_defers(alone: bool, queued_ahead: u32, live_clients: u32) -> bool {
    queued_ahead > 0 || (alone && live_clients > 0)
}

/// `north capacity quiet`: reads `{leases, queue, now}` and prints each quiet native lease with whether it still holds batch work and the batch jobs queued behind it.
pub(crate) fn run(idle_bound: f64) -> u8 {
    #[derive(Deserialize)]
    struct Input {
        leases: Vec<LeaseFact>,
        queue: Vec<QueueEntry>,
        now: f64,
    }
    let mut input = String::new();
    let parsed = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Input>(&input).map_err(|error| error.to_string()));
    let Input { leases, queue, now } = match parsed {
        Ok(input) => input,
        Err(message) => {
            eprintln!("north capacity quiet: {message}");
            return 2;
        }
    };
    println!("{}", Value::Array(standing(&leases, &queue, now, idle_bound)));
    0
}

/// The host's raw lease and queue records as the quiet standing of each quiet native lease.
pub(crate) fn standing_json(leases: &[Value], queue: &[Value], now: f64) -> Vec<Value> {
    let leases: Vec<LeaseFact> = leases.iter().filter_map(|lease| serde_json::from_value(lease.clone()).ok()).collect();
    let queue: Vec<QueueEntry> = queue.iter().filter_map(|entry| serde_json::from_value(entry.clone()).ok()).collect();
    standing(&leases, &queue, now, super::admission::quiet_idle_bound())
}

fn standing(leases: &[LeaseFact], queue: &[QueueEntry], now: f64, idle_bound: f64) -> Vec<Value> {
    let waiting: Vec<&QueueEntry> = queue.iter().filter(|entry| BATCH_CLASSES.contains(&entry.class.as_str())).collect();
    let longest = waiting.iter().map(|entry| ((now - entry.queued_at) / 1000.0).round()).fold(0.0, f64::max);
    leases
        .iter()
        .filter(|lease| lease.is_quiet_native())
        .map(|lease| {
            let view = lease.quiet_view(now);
            let holding = view.holds(idle_bound);
            let deferring = holding.then(|| json!({ "jobs": waiting.len(), "longestWaitingSeconds": longest }));
            json!({ "id": lease.id, "owner": view.owner, "heldSeconds": view.held_seconds, "idleSeconds": view.idle_seconds, "runSeconds": view.run_seconds, "holding": holding, "deferring": deferring })
        })
        .collect()
}
