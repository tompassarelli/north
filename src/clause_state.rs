use clause_package::{Term, decode_canonical_term_bytes};
use clause_runtime::{ExecutableSymbolV1, ExecutableValueV1};
use clause_workbench::ResidentSourceWorkbenchV1;

use crate::error::{NorthError, NorthResult};

const NORTH_SOURCE: &[u8] = include_bytes!("../clause/north.clause");
const MAX_EXACT_F64_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NorthPhase {
    Idle,
    Dispatching,
    Delegating,
    Settling,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationChange {
    Ready,
    Opening,
    Switching,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AttachmentIdentity(pub(crate) u64);

impl AttachmentIdentity {
    pub const fn number(self) -> u64 {
        self.0
    }

    fn argument(self) -> NorthResult<ExecutableValueV1> {
        if self.0 > MAX_EXACT_F64_INTEGER {
            return Err(NorthError::Protocol(
                "Clause attachment identity exhausted the exact F64 integer domain".into(),
            ));
        }
        ExecutableValueV1::number(self.0 as f64).map_err(|error| {
            NorthError::Protocol(format!(
                "attachment identity {} cannot enter Clause state: {error}",
                self.0
            ))
        })
    }
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
    conversation_change: ConversationChange,
    active_conversation: Option<String>,
    pending_conversation: Option<String>,
    draft_number: u64,
    next_attachment_number: u64,
    draft_attachments: Vec<AttachmentIdentity>,
    submitted_attachments: Vec<AttachmentIdentity>,
}

impl NorthState {
    pub fn open() -> NorthResult<Self> {
        let workbench = ResidentSourceWorkbenchV1::open(NORTH_SOURCE)?;
        let mut state = Self {
            workbench,
            phase: NorthPhase::Idle,
            active_delegated_child: None,
            terminal_delegated_child: None,
            conversation_change: ConversationChange::Ready,
            active_conversation: None,
            pending_conversation: None,
            draft_number: 1,
            next_attachment_number: 1,
            draft_attachments: Vec::new(),
            submitted_attachments: Vec::new(),
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

    pub fn active_conversation(&self) -> Option<&str> {
        self.active_conversation.as_deref()
    }

    pub const fn conversation_change(&self) -> ConversationChange {
        self.conversation_change
    }

    pub fn observe_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        let conversation = conversation_argument(conversation_id)?;
        self.transition(b"observe-conversation", &[conversation])
    }

    pub fn request_new_conversation(&mut self) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Ready)?;
        self.transition(b"request-new-conversation", &[])?;
        self.require_conversation_change(ConversationChange::Opening)
    }

    pub fn settle_new_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Opening)?;
        let conversation = conversation_argument(conversation_id)?;
        self.transition(b"settle-new-conversation", &[conversation])?;
        self.require_conversation_change(ConversationChange::Ready)?;
        self.require_active_conversation(conversation_id)
    }

    pub fn fail_new_conversation(&mut self) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Opening)?;
        self.transition(b"fail-new-conversation", &[])?;
        self.require_conversation_change(ConversationChange::Ready)
    }

    pub fn request_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Ready)?;
        let conversation = conversation_argument(conversation_id)?;
        self.transition(b"request-switch-conversation", &[conversation])?;
        self.require_conversation_change(ConversationChange::Switching)?;
        require_identity(
            "pending conversation",
            &self.pending_conversation,
            conversation_id,
        )
    }

    pub fn settle_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Switching)?;
        let conversation = conversation_argument(conversation_id)?;
        self.transition(b"settle-switch-conversation", &[conversation])?;
        self.require_conversation_change(ConversationChange::Ready)?;
        self.require_active_conversation(conversation_id)
    }

    pub fn fail_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Switching)?;
        let conversation = conversation_argument(conversation_id)?;
        self.transition(b"fail-switch-conversation", &[conversation])?;
        self.require_conversation_change(ConversationChange::Ready)
    }

    #[cfg(test)]
    pub fn terminal_delegated_child(&self) -> Option<&str> {
        self.terminal_delegated_child.as_deref()
    }

    pub fn attach_image(&mut self) -> NorthResult<AttachmentIdentity> {
        let identity = AttachmentIdentity(self.next_attachment_number);
        self.transition(b"attach-image", &[])?;
        if !self.draft_attachments.contains(&identity) {
            return Err(NorthError::Protocol(format!(
                "Clause did not project attached image {} in the active draft",
                identity.number()
            )));
        }
        Ok(identity)
    }

    pub fn detach_image(&mut self, identity: AttachmentIdentity) -> NorthResult<()> {
        self.transition(b"detach-image", &[identity.argument()?])?;
        if self.draft_attachments.contains(&identity) {
            return Err(NorthError::Protocol(format!(
                "Clause retained detached image {} in the active draft",
                identity.number()
            )));
        }
        Ok(())
    }

    pub fn submit(&mut self) -> NorthResult<Vec<AttachmentIdentity>> {
        let submitted = self.begin_turn(b"submit")?;
        self.require(NorthPhase::Dispatching)?;
        Ok(submitted)
    }

    pub fn settle_success(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.finish_turn(b"settle-success", &[])?;
        self.require(NorthPhase::Completed)
    }

    pub fn settle_failure(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.finish_turn(b"settle-failure", &[])?;
        self.require(NorthPhase::Failed)
    }

    pub fn delegate(&mut self) -> NorthResult<Vec<AttachmentIdentity>> {
        let submitted = self.begin_turn(b"delegate")?;
        self.require(NorthPhase::Delegating)?;
        Ok(submitted)
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
        self.finish_turn(b"settle-delegation-success", &[child])?;
        self.require(NorthPhase::Completed)?;
        self.require_terminal_child(child_id)
    }

    pub fn fail_delegation_before_child(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Delegating)?;
        self.finish_turn(b"fail-delegation-before-child", &[])?;
        self.require(NorthPhase::Failed)
    }

    pub fn fail_delegation_after_child(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Settling)?;
        let child = child_argument(child_id)?;
        self.finish_turn(b"fail-delegation-after-child", &[child])?;
        self.require(NorthPhase::Failed)?;
        self.require_terminal_child(child_id)
    }

    fn begin_turn(&mut self, designation: &'static [u8]) -> NorthResult<Vec<AttachmentIdentity>> {
        let draft_number = self.draft_number;
        let attachments = self.draft_attachments.clone();
        let mut transitions = Vec::with_capacity(attachments.len() * 2 + 1);
        for identity in &attachments {
            transitions.push((
                b"copy-submission-attachment".as_slice(),
                vec![identity.argument()?],
            ));
            transitions.push((b"detach-image".as_slice(), vec![identity.argument()?]));
        }
        transitions.push((designation, Vec::new()));
        self.transition_sequence(&transitions)?;
        if self.draft_number != draft_number + 1
            || !self.draft_attachments.is_empty()
            || self.submitted_attachments != attachments
        {
            return Err(NorthError::Protocol(
                "Clause did not atomically roll the draft into the submitted attachment set".into(),
            ));
        }
        Ok(attachments)
    }

    fn finish_turn(
        &mut self,
        designation: &'static [u8],
        arguments: &[ExecutableValueV1],
    ) -> NorthResult<()> {
        let attachments = self.submitted_attachments.clone();
        let mut transitions = Vec::with_capacity(attachments.len() + 1);
        for identity in attachments {
            transitions.push((
                b"clear-submitted-attachment".as_slice(),
                vec![identity.argument()?],
            ));
        }
        transitions.push((designation, arguments.to_vec()));
        self.transition_sequence(&transitions)?;
        if !self.submitted_attachments.is_empty() {
            return Err(NorthError::Protocol(
                "Clause retained attachments after turn settlement".into(),
            ));
        }
        Ok(())
    }

    fn transition(
        &mut self,
        designation: &[u8],
        arguments: &[ExecutableValueV1],
    ) -> NorthResult<()> {
        self.transition_sequence(&[(designation, arguments.to_vec())])
    }

    fn transition_sequence(
        &mut self,
        transitions: &[(&[u8], Vec<ExecutableValueV1>)],
    ) -> NorthResult<()> {
        let occurrences = transitions
            .iter()
            .map(|(designation, arguments)| {
                self.workbench.handler_occurrence(designation, arguments)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.workbench.run_occurrences_to_candidate(&occurrences)?;
        let admission = self.workbench.admit()?;
        let projection = decode_projection(&admission.projection.exact_term_bytes)?;
        self.phase = projection.phase;
        self.active_delegated_child = projection.active_delegated_child;
        self.terminal_delegated_child = projection.terminal_delegated_child;
        self.conversation_change = projection.conversation_change;
        self.active_conversation = projection.active_conversation;
        self.pending_conversation = projection.pending_conversation;
        self.draft_number = projection.draft_number;
        self.next_attachment_number = projection.next_attachment_number;
        self.draft_attachments = projection.draft_attachments;
        self.submitted_attachments = projection.submitted_attachments;
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
        require_identity(
            "active delegated child",
            &self.active_delegated_child,
            expected,
        )
    }

    fn require_terminal_child(&self, expected: &str) -> NorthResult<()> {
        require_identity(
            "terminal delegated child",
            &self.terminal_delegated_child,
            expected,
        )
    }

    fn require_conversation_change(&self, expected: ConversationChange) -> NorthResult<()> {
        if self.conversation_change == expected {
            return Ok(());
        }
        Err(NorthError::Protocol(format!(
            "conversation change projected {:?}, expected {:?}",
            self.conversation_change, expected
        )))
    }

    fn require_active_conversation(&self, expected: &str) -> NorthResult<()> {
        require_identity("active conversation", &self.active_conversation, expected)
    }
}

struct NorthProjection {
    phase: NorthPhase,
    active_delegated_child: Option<String>,
    terminal_delegated_child: Option<String>,
    conversation_change: ConversationChange,
    active_conversation: Option<String>,
    pending_conversation: Option<String>,
    draft_number: u64,
    next_attachment_number: u64,
    draft_attachments: Vec<AttachmentIdentity>,
    submitted_attachments: Vec<AttachmentIdentity>,
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
    let conversation_change =
        projected_symbol(projected_object_field(north, b"conversation-change")?)?;
    let conversation_change = match conversation_change {
        b"conversation-ready" => Ok(ConversationChange::Ready),
        b"conversation-opening" => Ok(ConversationChange::Opening),
        b"conversation-switching" => Ok(ConversationChange::Switching),
        other => Err(NorthError::Protocol(format!(
            "conversation state projected unknown conversation change {}",
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
        conversation_change,
        active_conversation: projected_conversation_identity(projected_object_field(
            north,
            b"active-conversation",
        )?)?,
        pending_conversation: projected_conversation_identity(projected_object_field(
            north,
            b"pending-conversation",
        )?)?,
        draft_number: projected_integer(projected_object_field(north, b"draft-number")?)?,
        next_attachment_number: projected_integer(projected_object_field(
            north,
            b"next-attachment-number",
        )?)?,
        draft_attachments: projected_attachment_set(projected_object_field(
            north,
            b"draft-attachment",
        )?)?,
        submitted_attachments: projected_attachment_set(projected_object_field(
            north,
            b"submitted-attachment",
        )?)?,
    })
}

fn child_argument(child_id: &str) -> NorthResult<ExecutableValueV1> {
    identity_argument("worker id", child_id)
}

fn conversation_argument(conversation_id: &str) -> NorthResult<ExecutableValueV1> {
    identity_argument("conversation id", conversation_id)
}

fn identity_argument(label: &str, identity: &str) -> NorthResult<ExecutableValueV1> {
    ExecutableSymbolV1::new(identity.as_bytes())
        .map(ExecutableValueV1::Symbol)
        .map_err(|error| {
            NorthError::Protocol(format!(
                "{label} cannot be represented in conversation state: {error}"
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

fn projected_conversation_identity(term: &Term) -> NorthResult<Option<String>> {
    let identity = projected_symbol(term)?;
    if identity == b"no-conversation" {
        return Ok(None);
    }
    String::from_utf8(identity.to_vec()).map(Some).map_err(|_| {
        NorthError::Protocol("conversation state projected a non-UTF-8 conversation id".into())
    })
}

fn require_identity(label: &str, observed: &Option<String>, expected: &str) -> NorthResult<()> {
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

fn projected_integer(term: &Term) -> NorthResult<u64> {
    let atom = term.as_atom().ok_or_else(|| {
        NorthError::Protocol("conversation state projected a non-numeric value".into())
    })?;
    if atom.kind() != b"clause/process-projected-f64-v1" {
        return Err(NorthError::Protocol(
            "conversation state projected a non-numeric value".into(),
        ));
    }
    let bytes: [u8; 8] = atom
        .canonical_payload()
        .try_into()
        .map_err(|_| NorthError::Protocol("projected F64 payload is not exact".into()))?;
    let value = f64::from_bits(u64::from_le_bytes(bytes));
    if !value.is_finite()
        || value.fract() != 0.0
        || !(1.0..=(MAX_EXACT_F64_INTEGER as f64)).contains(&value)
    {
        return Err(NorthError::Protocol(format!(
            "conversation state projected invalid positive integer {value}"
        )));
    }
    Ok(value as u64)
}

fn projected_attachment_set(term: &Term) -> NorthResult<Vec<AttachmentIdentity>> {
    let [header, tree, end] = term
        .as_triple()
        .ok_or_else(|| {
            NorthError::Protocol("conversation state projected an untyped attachment set".into())
        })?
        .slots();
    let header = header.as_atom().ok_or_else(|| {
        NorthError::Protocol("conversation state projected a non-atom set header".into())
    })?;
    if header.kind() != b"clause/process-projected-set-v1"
        || header.canonical_payload() != [0]
        || end.as_atom().is_none_or(|end| {
            end.kind() != b"clause/process-projected-set-end-v1"
                || !end.canonical_payload().is_empty()
        })
    {
        return Err(NorthError::Protocol(
            "conversation state projected an invalid numeric set wrapper".into(),
        ));
    }

    fn collect(term: &Term, values: &mut Vec<AttachmentIdentity>) -> NorthResult<()> {
        if let Some(end) = term.as_atom() {
            if end.kind() == b"clause/process-projected-set-end-v1"
                && end.canonical_payload().is_empty()
            {
                return Ok(());
            }
            return Err(NorthError::Protocol(
                "conversation state projected an invalid set terminator".into(),
            ));
        }
        let [left, value, right] = term
            .as_triple()
            .ok_or_else(|| {
                NorthError::Protocol("conversation state projected an invalid set tree".into())
            })?
            .slots();
        collect(left, values)?;
        values.push(AttachmentIdentity(projected_integer(value)?));
        collect(right, values)
    }

    let mut values = Vec::new();
    collect(tree, &mut values)?;
    Ok(values)
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
    fn clause_owns_image_identity_draft_rollover_and_submission_membership() {
        let mut state = NorthState::open().expect("North Clause source opens");
        let first = state.attach_image().expect("first image is admitted");
        let second = state.attach_image().expect("second image is admitted");
        assert_eq!(first.number(), 1);
        assert_eq!(second.number(), 2);
        assert_eq!(state.draft_attachments, vec![first, second]);

        state.detach_image(first).expect("first image is detached");
        assert_eq!(state.draft_attachments, vec![second]);

        let submitted = state.submit().expect("draft submission is admitted");
        assert_eq!(submitted, vec![second]);
        assert_eq!(state.draft_number, 2);
        assert!(state.draft_attachments.is_empty());
        assert_eq!(state.submitted_attachments, vec![second]);

        state.settle_success().expect("image turn settles");
        assert!(state.submitted_attachments.is_empty());
        let third = state.attach_image().expect("next draft accepts an image");
        assert_eq!(third.number(), 3);
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

    #[test]
    fn clause_owns_new_and_switched_conversation_identity() {
        const FIRST: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a1";
        const SECOND: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        const THIRD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a3";
        let mut state = NorthState::open().expect("North Clause source opens");

        state
            .request_new_conversation()
            .expect("new conversation effect is admitted");
        assert_eq!(state.conversation_change(), ConversationChange::Opening);
        state
            .settle_new_conversation(FIRST)
            .expect("new conversation receipt settles");
        assert_eq!(state.active_conversation(), Some(FIRST));

        state
            .observe_conversation(SECOND)
            .expect("foreign conversation discovery is observed");
        state
            .observe_conversation(THIRD)
            .expect("the picker can move to a later resume candidate");
        state
            .request_switch_conversation(SECOND)
            .expect("an earlier retained conversation remains selectable");
        assert_eq!(state.conversation_change(), ConversationChange::Switching);
        state
            .settle_switch_conversation(SECOND)
            .expect("resumed conversation receipt settles");
        assert_eq!(state.active_conversation(), Some(SECOND));
    }

    #[test]
    fn clause_rejects_unknown_switch_and_preserves_active_conversation_on_failure() {
        const FIRST: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a1";
        const SECOND: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        let mut state = NorthState::open().expect("North Clause source opens");
        state.request_new_conversation().unwrap();
        state.settle_new_conversation(FIRST).unwrap();

        state
            .request_switch_conversation(SECOND)
            .expect_err("an unobserved conversation cannot be selected");
        assert_eq!(state.conversation_change(), ConversationChange::Ready);
        assert_eq!(state.active_conversation(), Some(FIRST));

        state.observe_conversation(SECOND).unwrap();
        state.request_switch_conversation(SECOND).unwrap();
        state
            .fail_switch_conversation(SECOND)
            .expect("failed resume returns to the prior conversation");
        assert_eq!(state.conversation_change(), ConversationChange::Ready);
        assert_eq!(state.active_conversation(), Some(FIRST));
    }
}
