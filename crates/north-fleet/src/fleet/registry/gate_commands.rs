use std::collections::BTreeMap;

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{self, Closure, Decision, Gate, GateRequest, Hold, default_overdue, settle};
use crate::fleet::model::Delivery;
use crate::fleet::runtime::now_ms;

use super::Fleet;

impl Fleet {
    pub fn open_gate(&self, request: &GateRequest) -> NorthResult<i64> {
        let id = self.record_gate(request)?;
        if request.has_default() {
            (self.arm)(id)?;
        }
        Ok(id)
    }

    pub(super) fn record_gate(&self, request: &GateRequest) -> NorthResult<i64> {
        let asker = self.record(&request.session)?;
        let request = GateRequest { verification: asker.verification, ..request.clone() };
        let abroad = asker.parent.as_deref().is_some_and(|parent| self.record(parent).is_err()).then(|| self.host.clone());
        gates::open(self.ledger.as_ref(), &request, asker.parent.as_deref(), abroad.as_deref(), now_ms(), |parent, text| self.tell(parent, text, Delivery::Queued))
    }

    pub fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.ledger.gate(id)
    }

    pub fn decide(&self, id: i64, decision: Decision) -> NorthResult<Closure> {
        settle(self.ledger.as_ref(), id, decision, now_ms(), |session, text| self.answer_session(session, text))
    }

    pub fn answer_gate(&self, id: i64, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<Closure> {
        self.answer_gate_with(id, number, note, caller, |session, text| self.answer_session(session, text))
    }

    pub(super) fn answer_gate_with(&self, id: i64, number: usize, note: Option<&str>, caller: Option<&str>, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
        let gate = self.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
        if gate.closure.is_some() {
            return settle(self.ledger.as_ref(), id, Decision::Option(number), now_ms(), deliver);
        }
        let asker = self.record(&gate.request.session)?;
        gates::answer_gate(self.ledger.as_ref(), id, number, note, gates::Authority { caller, parent: asker.parent.as_deref() }, now_ms(), deliver)
    }

    /// Each open decision gate that names an issue, here or on another host as the cached fleet table holds it.
    pub fn gate_holds(&self) -> NorthResult<Vec<Hold>> {
        let here = gates::decisions(self.ledger.gates(u64::MAX)?).into_iter().filter_map(|gate| Some(Hold { issue: gate.request.issue?, box_text: gate.request.box_text, gate: gate.id.to_string() }));
        let elsewhere = self.gates_elsewhere()?.into_iter().filter_map(|gate| Some(Hold { issue: gate.request.issue.clone()?, box_text: gate.request.box_text.clone(), gate: gate.label() }));
        Ok(elsewhere.chain(here).collect())
    }

    /// Each issue an open decision gate decides whole, here or on another host as the cached fleet table holds it, with the gate's id; a gate scoped to one box leaves the issue's other boxes free; see [`Self::gate_holds`].
    pub fn gated_issues(&self) -> NorthResult<BTreeMap<String, String>> {
        Ok(self.gate_holds()?.into_iter().filter(|hold| hold.box_text.is_none()).map(|hold| (hold.issue, hold.gate)).collect())
    }

    pub fn default_overdue_gates(&self) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
        default_overdue(self.ledger.as_ref(), now_ms(), |session, text| self.answer_session(session, text))
    }
}
