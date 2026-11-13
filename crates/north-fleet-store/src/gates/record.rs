use crate::error::NorthResult;
use crate::gate_lifecycle::GatePhase;

use super::request::GateRequest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Answerer {
    Session(String),
    Tom,
    Default,
    Orphaned,
    Ticked,
}

impl Answerer {
    pub fn label(&self) -> &str {
        match self {
            Answerer::Session(name) => name,
            Answerer::Tom => "tom",
            Answerer::Default => "default",
            Answerer::Orphaned => "orphaned",
            Answerer::Ticked => "ticked",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "tom" => Some(Answerer::Tom),
            "default" => Some(Answerer::Default),
            "orphaned" => Some(Answerer::Orphaned),
            "ticked" => Some(Answerer::Ticked),
            name if !name.is_empty() => Some(Answerer::Session(name.to_owned())),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Closure {
    pub answer: String,
    pub by: Answerer,
    pub closed_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gate {
    pub id: i64,
    pub request: GateRequest,
    pub opened_ms: u64,
    pub phase: GatePhase,
    pub closure: Option<Closure>,
}

impl Gate {
    /// A gate as a book without lifecycle records holds it: its phase follows from whether it has closed.
    pub fn new(id: i64, request: GateRequest, opened_ms: u64, closure: Option<Closure>) -> Self {
        let phase = closure.as_ref().map_or_else(|| GatePhase::opened(&request), |closure| GatePhase::settled(&closure.by));
        Self { id, request, opened_ms, phase, closure }
    }

    pub fn default_at_ms(&self) -> Option<u64> {
        self.request.has_default().then(|| self.opened_ms.saturating_add(self.request.default_after_ms))
    }

    pub fn is_open(&self) -> bool {
        self.phase.is_open()
    }

    pub fn for_tom(&self) -> bool {
        self.request.for_tom()
    }

    pub fn option(&self, number: usize) -> Option<&str> {
        self.request.options.get(number.checked_sub(1)?).map(String::as_str)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Option(usize),
    Text(String),
    Default,
}

pub fn decisions(gates: Vec<Gate>) -> Vec<Gate> {
    gates.into_iter().filter(|gate| !gate.request.verification).collect()
}

pub trait GateBook: Send + Sync {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64>;
    fn gate(&self, id: i64) -> NorthResult<Option<Gate>>;
    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>>;
    /// Closes an open gate with `closure` and starts delivering its answer; false when another closer got there first.
    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool>;
    /// Moves a gate to `to` for `cause`; false when its phase or a live holder refuses the move.
    fn move_gate(&self, id: i64, to: GatePhase, cause: &str, at_ms: u64) -> NorthResult<bool>;
}
