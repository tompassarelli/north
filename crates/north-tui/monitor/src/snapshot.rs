use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reading<T> {
    pub at: Option<u64>,
    pub value: Option<T>,
    pub error: Option<String>,
}

impl<T> Default for Reading<T> {
    fn default() -> Self {
        Self { at: None, value: None, error: None }
    }
}

impl<T> Reading<T> {
    pub fn ok(at: u64, value: T) -> Self {
        Self { at: Some(at), value: Some(value), error: None }
    }

    pub fn failed(at: u64, error: impl Into<String>) -> Self {
        Self { at: Some(at), value: None, error: Some(error.into()) }
    }

    pub fn update(&mut self, at: u64, result: Result<T, String>) {
        match result {
            Ok(value) => *self = Self::ok(at, value),
            Err(error) => {
                self.error = Some(error);
                if self.value.is_none() {
                    self.at = Some(at);
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub at: u64,
    pub fleet: Reading<Fleet>,
    pub landing: Reading<Landing>,
    pub landers: Reading<Vec<Lander>>,
    pub hosts: Vec<Host>,
    pub lag: Reading<BTreeMap<String, BTreeMap<String, u32>>>,
    pub rollout: Reading<Rollout>,
    pub health: Reading<Health>,
    pub builds: Reading<Builds>,
    pub ladder: Reading<u8>,
    pub holds: Reading<Holds>,
    pub trends: Trends,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fleet {
    pub source: String,
    pub servers: BTreeMap<String, BTreeMap<String, u32>>,
}

pub const BUSY_STATES: [&str; 2] = ["working", "starting"];
pub const IDLE_STATES: [&str; 2] = ["idle", "parked"];

impl Fleet {
    pub fn count(&self, state: &str) -> u32 {
        self.servers.values().filter_map(|states| states.get(state)).sum()
    }

    pub fn busy(&self) -> u32 {
        BUSY_STATES.iter().map(|state| self.count(state)).sum()
    }

    pub fn idle(&self) -> u32 {
        IDLE_STATES.iter().map(|state| self.count(state)).sum()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Landing {
    pub depth: u32,
    pub depth_5m_ago: u32,
    pub land_p50_s: Option<u64>,
    pub land_p95_s: Option<u64>,
    pub landed_last_hour: u32,
    pub decided_6h: u32,
    pub failed_6h: u32,
    pub recent: Vec<Decided>,
    pub depth_trend: Vec<u32>,
    pub landed_per_hour: Vec<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Decided {
    pub repo: String,
    pub branch: String,
    pub verdict: String,
    pub at: u64,
    pub seconds: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Lander {
    pub repo: String,
    pub up: bool,
    pub build: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub state: Reading<HostState>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HostState {
    pub runtime: Option<String>,
    pub follow: Option<String>,
    pub load1: Option<f64>,
    pub cores: Option<u32>,
}

impl HostState {
    pub fn load_per_core(&self) -> Option<f64> {
        Some(self.load1? / f64::from(self.cores?.max(1)))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rollout {
    pub promote_state: Option<String>,
    pub promote_sha: Option<String>,
    pub promote_at: Option<u64>,
    pub rollbacks_24h: u32,
    pub last_rollback: Option<(String, u64)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Health {
    pub source: String,
    pub written: u64,
    pub green: u32,
    pub amber: Vec<String>,
    pub red: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HoldRow {
    pub host: String,
    pub kind: String,
    pub set_by: String,
    pub why: String,
    pub lifts_when: String,
    pub red: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Holds {
    pub holds: Vec<HoldRow>,
    pub unread: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Builds {
    pub compile_s: Option<f64>,
    pub test_s: Option<f64>,
    pub land_checks_s: Option<f64>,
    pub ended_at: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Trends {
    pub busy: Vec<u32>,
    pub load: Vec<f64>,
    pub sampled_at: u64,
}

pub const TREND_POINTS: usize = 60;
pub const TREND_STEP_S: u64 = 10;

impl Trends {
    pub fn sample(&mut self, at: u64, busy: Option<u32>, load: Option<f64>) {
        if at < self.sampled_at + TREND_STEP_S {
            return;
        }
        self.sampled_at = at;
        if let Some(busy) = busy {
            push(&mut self.busy, busy);
        }
        if let Some(load) = load {
            push(&mut self.load, load);
        }
    }
}

fn push<T>(ring: &mut Vec<T>, value: T) {
    ring.push(value);
    if ring.len() > TREND_POINTS {
        ring.remove(0);
    }
}
