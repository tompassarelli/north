use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{self, Closure, Decision, Gate, GateRequest, default_overdue, settle};
use crate::fleet::model::Delivery;
use crate::fleet::relay;
use crate::fleet::runtime::now_ms;

use super::{Fleet, parents};

impl Fleet {
    pub fn open_gate(&self, request: &GateRequest) -> NorthResult<i64> {
        self.open_gate_arming(request, &parents::await_default)
    }

    pub(super) fn open_gate_arming(&self, request: &GateRequest, arm: &dyn Fn(i64) -> NorthResult<()>) -> NorthResult<i64> {
        let asker = self.record(&request.session)?;
        let request = GateRequest { verification: asker.verification, ..request.clone() };
        let abroad = asker.parent.as_deref().is_some_and(|parent| self.record(parent).is_err()).then(relay::hostname);
        let id = gates::open(self.ledger.as_ref(), &request, asker.parent.as_deref(), abroad.as_deref(), now_ms(), |parent, text| self.tell(parent, text, Delivery::Queued))?;
        arm(id)?;
        Ok(id)
    }

    pub fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.ledger.gate(id)
    }

    pub fn decide(&self, id: i64, decision: Decision) -> NorthResult<Closure> {
        settle(self.ledger.as_ref(), id, decision, now_ms(), |session, text| self.send(session, text, Delivery::Now))
    }

    pub fn answer_gate(&self, id: i64, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<Closure> {
        let gate = self.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
        if gate.closure.is_some() {
            return self.decide(id, Decision::Option(number));
        }
        let asker = self.record(&gate.request.session)?;
        gates::answer_gate(self.ledger.as_ref(), id, number, note, gates::Authority { caller, parent: asker.parent.as_deref() }, now_ms(), |session, text| self.send(session, text, Delivery::Now))
    }

    pub fn default_overdue_gates(&self) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
        default_overdue(self.ledger.as_ref(), now_ms(), |session, text| self.send(session, text, Delivery::Now))
    }
}
