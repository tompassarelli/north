use clause_package::{Term, decode_canonical_term_bytes};
use clause_runtime::{ExecutableSymbolV1, ExecutableValueV1};
use clause_workbench::ResidentSourceWorkbenchV1;

use crate::error::{NorthError, NorthResult};

const NORTH_SOURCE: &[u8] = include_bytes!("../clause/north.clause");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NorthPhase {
    Idle,
    Dispatching,
    Delegating,
    Settling,
    Completed,
    Failed,
}

impl NorthPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Dispatching => "dispatching",
            Self::Delegating => "delegating",
            Self::Settling => "settling",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

pub struct NorthState {
    workbench: ResidentSourceWorkbenchV1,
    phase: NorthPhase,
    active_delegated_child: Option<String>,
    terminal_delegated_child: Option<String>,
}

impl NorthState {
    pub fn open() -> NorthResult<Self> {
        let workbench = ResidentSourceWorkbenchV1::open(NORTH_SOURCE)?;
        let mut state = Self {
            workbench,
            phase: NorthPhase::Idle,
            active_delegated_child: None,
            terminal_delegated_child: None,
        };
        state.transition(b"inspect", &[])?;
        if state.phase != NorthPhase::Idle {
            return Err(NorthError::Protocol(format!(
                "fresh conversation state projected {}, expected idle",
                state.phase.label()
            )));
        }
        Ok(state)
    }

    pub const fn phase(&self) -> NorthPhase {
        self.phase
    }

    pub fn active_delegated_child(&self) -> Option<&str> {
        self.active_delegated_child.as_deref()
    }

    #[cfg(test)]
    pub fn terminal_delegated_child(&self) -> Option<&str> {
        self.terminal_delegated_child.as_deref()
    }

    pub fn submit(&mut self) -> NorthResult<()> {
        self.transition(b"submit", &[])?;
        self.require(NorthPhase::Dispatching)
    }

    pub fn settle_success(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.transition(b"settle-success", &[])?;
        self.require(NorthPhase::Completed)
    }

    pub fn settle_failure(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.transition(b"settle-failure", &[])?;
        self.require(NorthPhase::Failed)
    }

    pub fn delegate(&mut self) -> NorthResult<()> {
        self.transition(b"delegate", &[])?;
        self.require(NorthPhase::Delegating)
    }

    pub fn child_spawned(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Delegating)?;
        let child = child_argument(child_id)?;
        self.transition(b"child-spawned", &[child])?;
        self.require(NorthPhase::Settling)?;
        self.require_active_child(child_id)
    }

    pub fn settle_delegation_success(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Settling)?;
        let child = child_argument(child_id)?;
        self.transition(b"settle-delegation-success", &[child])?;
        self.require(NorthPhase::Completed)?;
        self.require_terminal_child(child_id)
    }

    pub fn fail_delegation_before_child(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Delegating)?;
        self.transition(b"fail-delegation-before-child", &[])?;
        self.require(NorthPhase::Failed)
    }

    pub fn fail_delegation_after_child(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Settling)?;
        let child = child_argument(child_id)?;
        self.transition(b"fail-delegation-after-child", &[child])?;
        self.require(NorthPhase::Failed)?;
        self.require_terminal_child(child_id)
    }

    fn transition(
        &mut self,
        designation: &[u8],
        arguments: &[ExecutableValueV1],
    ) -> NorthResult<()> {
        let occurrence = self.workbench.handler_occurrence(designation, arguments)?;
        self.workbench.run_occurrences_to_candidate(&[occurrence])?;
        let admission = self.workbench.admit()?;
        let projection = decode_projection(&admission.projection.exact_term_bytes)?;
        self.phase = projection.phase;
        self.active_delegated_child = projection.active_delegated_child;
        self.terminal_delegated_child = projection.terminal_delegated_child;
        Ok(())
    }

    fn require(&self, expected: NorthPhase) -> NorthResult<()> {
        if self.phase == expected {
            return Ok(());
        }
        Err(NorthError::Protocol(format!(
            "conversation state projected {}, expected {}",
            self.phase.label(),
            expected.label()
        )))
    }

    fn require_active_child(&self, expected: &str) -> NorthResult<()> {
        require_child_identity(
            "active delegated child",
            &self.active_delegated_child,
            expected,
        )
    }

    fn require_terminal_child(&self, expected: &str) -> NorthResult<()> {
        require_child_identity(
            "terminal delegated child",
            &self.terminal_delegated_child,
            expected,
        )
    }
}

struct NorthProjection {
    phase: NorthPhase,
    active_delegated_child: Option<String>,
    terminal_delegated_child: Option<String>,
}

fn decode_projection(exact_term_bytes: &[u8]) -> NorthResult<NorthProjection> {
    let term = decode_canonical_term_bytes(exact_term_bytes).map_err(|error| {
        NorthError::Protocol(format!("conversation state did not decode: {error}"))
    })?;
    let north = projected_object_field(&term, b"north-main")?;
    let phase = projected_symbol(projected_object_field(north, b"phase")?)?;
    let phase = match phase {
        b"idle" => Ok(NorthPhase::Idle),
        b"dispatching" => Ok(NorthPhase::Dispatching),
        b"delegating" => Ok(NorthPhase::Delegating),
        b"settling" => Ok(NorthPhase::Settling),
        b"completed" => Ok(NorthPhase::Completed),
        b"failed" => Ok(NorthPhase::Failed),
        other => Err(NorthError::Protocol(format!(
            "conversation state projected unknown North phase {}",
            String::from_utf8_lossy(other)
        ))),
    }?;
    Ok(NorthProjection {
        phase,
        active_delegated_child: projected_child_identity(projected_object_field(
            north,
            b"active-delegated-child",
        )?)?,
        terminal_delegated_child: projected_child_identity(projected_object_field(
            north,
            b"terminal-delegated-child",
        )?)?,
    })
}

fn child_argument(child_id: &str) -> NorthResult<ExecutableValueV1> {
    ExecutableSymbolV1::new(child_id.as_bytes())
        .map(ExecutableValueV1::Symbol)
        .map_err(|error| {
            NorthError::Protocol(format!(
                "worker id cannot be represented in conversation state: {error}"
            ))
        })
}

fn projected_child_identity(term: &Term) -> NorthResult<Option<String>> {
    let identity = projected_symbol(term)?;
    if identity == b"no-agent-run" {
        return Ok(None);
    }
    String::from_utf8(identity.to_vec()).map(Some).map_err(|_| {
        NorthError::Protocol("conversation state projected a non-UTF-8 worker id".into())
    })
}

fn require_child_identity(
    label: &str,
    observed: &Option<String>,
    expected: &str,
) -> NorthResult<()> {
    if observed.as_deref() == Some(expected) {
        return Ok(());
    }
    Err(NorthError::Protocol(format!(
        "conversation state projected {label} {:?}, expected {expected}",
        observed.as_deref()
    )))
}

fn projected_object_field<'a>(term: &'a Term, expected: &[u8]) -> NorthResult<&'a Term> {
    let mut current = term;
    loop {
        let [field, value, rest] = current
            .as_triple()
            .ok_or_else(|| {
                NorthError::Protocol(format!(
                    "conversation state lacks field {}",
                    String::from_utf8_lossy(expected)
                ))
            })?
            .slots();
        let field = field.as_atom().ok_or_else(|| {
            NorthError::Protocol("conversation state projected a non-atom field".into())
        })?;
        if field.canonical_payload() == expected {
            return Ok(value);
        }
        current = rest;
    }
}

fn projected_symbol(term: &Term) -> NorthResult<&[u8]> {
    let atom = term.as_atom().ok_or_else(|| {
        NorthError::Protocol("conversation state projected a non-symbol phase".into())
    })?;
    if atom.kind() != b"clause/process-projected-symbol-v1" {
        return Err(NorthError::Protocol(
            "conversation state projected a non-symbol value".into(),
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
    fn clause_admits_repeated_turns_after_success_and_failure() {
        let mut state = NorthState::open().expect("North Clause source opens");
        state.submit().expect("first submit is admitted");
        state.settle_success().expect("first turn settles");
        state.submit().expect("second submit is admitted");
        state.settle_failure().expect("second turn settles");
        state
            .submit()
            .expect("third submit is admitted after failure");
        assert_eq!(state.phase(), NorthPhase::Dispatching);
    }

    #[test]
    fn clause_owns_failure_settlement() {
        let mut state = NorthState::open().expect("North Clause source opens");
        state.submit().expect("submit is admitted");
        state.settle_failure().expect("failure is admitted");
        assert_eq!(state.phase(), NorthPhase::Failed);
    }

    #[test]
    fn clause_owns_delegation_admission_and_settlement() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        let mut state = NorthState::open().expect("North Clause source opens");
        state.delegate().expect("delegation is admitted");
        assert_eq!(state.phase(), NorthPhase::Delegating);
        state
            .child_spawned(CHILD)
            .expect("child receipt is admitted");
        assert_eq!(state.phase(), NorthPhase::Settling);
        assert_eq!(state.active_delegated_child(), Some(CHILD));
        state
            .settle_delegation_success(CHILD)
            .expect("terminal child settles delegation");
        assert_eq!(state.phase(), NorthPhase::Completed);
        assert_eq!(state.active_delegated_child(), None);
        assert_eq!(state.terminal_delegated_child(), Some(CHILD));

        state.delegate().expect("another delegation is admitted");
        assert_eq!(state.phase(), NorthPhase::Delegating);
        assert_eq!(state.terminal_delegated_child(), None);
    }

    #[test]
    fn clause_owns_delegation_failure_before_and_after_spawn() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        let mut before = NorthState::open().expect("North Clause source opens");
        before.delegate().expect("delegation is admitted");
        before
            .fail_delegation_before_child()
            .expect("pre-child failure is admitted");
        assert_eq!(before.phase(), NorthPhase::Failed);

        let mut after = NorthState::open().expect("North Clause source opens");
        after.delegate().expect("delegation is admitted");
        after
            .child_spawned(CHILD)
            .expect("child receipt is admitted");
        after
            .fail_delegation_after_child(CHILD)
            .expect("post-child failure is admitted");
        assert_eq!(after.phase(), NorthPhase::Failed);
        assert_eq!(after.active_delegated_child(), None);
        assert_eq!(after.terminal_delegated_child(), Some(CHILD));
    }

    #[test]
    fn clause_rejects_settlement_for_a_different_child() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        const OTHER: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a3";
        let mut state = NorthState::open().expect("North Clause source opens");
        state.delegate().expect("delegation is admitted");
        state
            .child_spawned(CHILD)
            .expect("child receipt is admitted");

        let error = state
            .settle_delegation_success(OTHER)
            .expect_err("another child cannot settle this delegation");
        assert!(
            error
                .to_string()
                .contains("projected settling, expected completed")
        );
        assert_eq!(state.phase(), NorthPhase::Settling);
        assert_eq!(state.active_delegated_child(), Some(CHILD));
        assert_eq!(state.terminal_delegated_child(), None);
    }
}
