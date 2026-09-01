use clause_package::{Term, decode_canonical_term_bytes};
use clause_workbench::ResidentSourceWorkbenchV1;

use crate::error::{NorthError, NorthResult};

const NORTH_SOURCE: &[u8] = include_bytes!("../clause/north.clause");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NorthPhase {
    Idle,
    Dispatching,
    Completed,
    Failed,
}

impl NorthPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Dispatching => "dispatching",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

pub struct NorthState {
    workbench: ResidentSourceWorkbenchV1,
    phase: NorthPhase,
}

impl NorthState {
    pub fn open() -> NorthResult<Self> {
        let workbench = ResidentSourceWorkbenchV1::open(NORTH_SOURCE)?;
        let mut state = Self {
            workbench,
            phase: NorthPhase::Idle,
        };
        state.phase = state.transition(b"inspect")?;
        if state.phase != NorthPhase::Idle {
            return Err(NorthError::Protocol(format!(
                "fresh Clause state projected {}, expected idle",
                state.phase.label()
            )));
        }
        Ok(state)
    }

    pub const fn phase(&self) -> NorthPhase {
        self.phase
    }

    pub fn submit(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Idle)?;
        self.phase = self.transition(b"submit")?;
        self.require(NorthPhase::Dispatching)
    }

    pub fn settle_success(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.phase = self.transition(b"settle-success")?;
        self.require(NorthPhase::Completed)
    }

    pub fn settle_failure(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.phase = self.transition(b"settle-failure")?;
        self.require(NorthPhase::Failed)
    }

    fn transition(&mut self, designation: &[u8]) -> NorthResult<NorthPhase> {
        let occurrence = self.workbench.handler_occurrence(designation, &[])?;
        self.workbench.run_occurrences_to_candidate(&[occurrence])?;
        let admission = self.workbench.admit()?;
        decode_phase(&admission.projection.exact_term_bytes)
    }

    fn require(&self, expected: NorthPhase) -> NorthResult<()> {
        if self.phase == expected {
            return Ok(());
        }
        Err(NorthError::Protocol(format!(
            "Clause projected {}, expected {}",
            self.phase.label(),
            expected.label()
        )))
    }
}

fn decode_phase(exact_term_bytes: &[u8]) -> NorthResult<NorthPhase> {
    let term = decode_canonical_term_bytes(exact_term_bytes).map_err(|error| {
        NorthError::Protocol(format!("Clause projection did not decode: {error}"))
    })?;
    let north = projected_object_field(&term, b"north-main")?;
    let phase = projected_symbol(projected_object_field(north, b"phase")?)?;
    match phase {
        b"idle" => Ok(NorthPhase::Idle),
        b"dispatching" => Ok(NorthPhase::Dispatching),
        b"completed" => Ok(NorthPhase::Completed),
        b"failed" => Ok(NorthPhase::Failed),
        other => Err(NorthError::Protocol(format!(
            "Clause projected unknown North phase {}",
            String::from_utf8_lossy(other)
        ))),
    }
}

fn projected_object_field<'a>(term: &'a Term, expected: &[u8]) -> NorthResult<&'a Term> {
    let mut current = term;
    loop {
        let [field, value, rest] = current
            .as_triple()
            .ok_or_else(|| {
                NorthError::Protocol(format!(
                    "Clause projection lacks field {}",
                    String::from_utf8_lossy(expected)
                ))
            })?
            .slots();
        let field = field
            .as_atom()
            .ok_or_else(|| NorthError::Protocol("Clause projected a non-atom field".into()))?;
        if field.canonical_payload() == expected {
            return Ok(value);
        }
        current = rest;
    }
}

fn projected_symbol(term: &Term) -> NorthResult<&[u8]> {
    let atom = term
        .as_atom()
        .ok_or_else(|| NorthError::Protocol("Clause projected a non-symbol phase".into()))?;
    if atom.kind() != b"clause/process-projected-symbol-v1" {
        return Err(NorthError::Protocol(
            "Clause projected a non-symbol phase value".into(),
        ));
    }
    Ok(atom.canonical_payload())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clause_owns_submit_and_settlement() {
        let mut state = NorthState::open().expect("North Clause source opens");
        assert_eq!(state.phase(), NorthPhase::Idle);
        state.submit().expect("submit is admitted");
        assert_eq!(state.phase(), NorthPhase::Dispatching);
        state.settle_success().expect("success is admitted");
        assert_eq!(state.phase(), NorthPhase::Completed);
    }

    #[test]
    fn clause_owns_failure_settlement() {
        let mut state = NorthState::open().expect("North Clause source opens");
        state.submit().expect("submit is admitted");
        state.settle_failure().expect("failure is admitted");
        assert_eq!(state.phase(), NorthPhase::Failed);
    }
}
