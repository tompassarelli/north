use crate::error::NorthResult;

use super::gates::{Gate, GateBook};
use super::receipts::ReceiptBook;
use super::turns::RunSink;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionSpend {
    pub session: String,
    pub tokens: u64,
    pub api_usd: Option<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Landing {
    pub task_ref: String,
    pub sha: String,
    pub wall_ms: Option<u64>,
    pub landed_ms: u64,
}

pub trait Books: Send + Sync {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>>;
    fn landed_since(&self, since_ms: u64) -> NorthResult<Vec<Landing>>;
    fn record_landing(&self, task_ref: &str, sha: &str, landed_ms: u64, wall_ms: Option<u64>, review: Option<&str>) -> NorthResult<bool>;
}

pub trait WatchBooks: RunSink + Books + ReceiptBook {}

impl<T: RunSink + Books + ReceiptBook> WatchBooks for T {}

pub trait FleetBooks: WatchBooks + GateBook {}

impl<T: WatchBooks + GateBook> FleetBooks for T {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Digest {
    pub gates: Vec<Gate>,
    pub spend: Vec<SessionSpend>,
    pub landed: Vec<Landing>,
}

impl Digest {
    pub fn spend_of(&self, session: &str) -> Option<&SessionSpend> {
        self.spend.iter().find(|spend| spend.session == session)
    }

    pub fn open_gates(&self) -> impl Iterator<Item = &Gate> {
        self.gates.iter().filter(|gate| gate.is_open())
    }
}
