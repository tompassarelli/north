use clause_package::{Term, decode_canonical_term_bytes};
use clause_runtime::{
    ExecutableReferentV1, ExecutableRelationTableV1, ExecutableValueV1,
    projected_relation_table_v1, projected_text_value_v1,
};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Goal {
    identity: ExecutableReferentV1,
    title: String,
    objective: String,
    status: String,
    order: u64,
    prior_objectives: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    name: String,
    description: String,
    handler: String,
    order: u64,
}

impl CommandSpec {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEffect {
    action: String,
    payload: String,
}

impl HostEffect {
    pub fn action(&self) -> &str {
        &self.action
    }

    pub fn payload(&self) -> &str {
        &self.payload
    }
}

impl Goal {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn objective(&self) -> &str {
        &self.objective
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub const fn order(&self) -> u64 {
        self.order
    }

    pub fn prior_objectives(&self) -> &[String] {
        &self.prior_objectives
    }
}

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
    goals: Vec<Goal>,
    active_goal: Option<ExecutableReferentV1>,
    commands: Vec<CommandSpec>,
    input_handler: String,
    host_effect: String,
    effect_payload: String,
    notice: String,
    active_view: String,
    next_view_handler: String,
    previous_view_handler: String,
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
            goals: Vec::new(),
            active_goal: None,
            commands: Vec::new(),
            input_handler: "submit-input".into(),
            host_effect: String::new(),
            effect_payload: String::new(),
            notice: String::new(),
            active_view: "agents".into(),
            next_view_handler: "view-agents-next".into(),
            previous_view_handler: "view-agents-previous".into(),
        };
        state.transition(b"initialize", &[])?;
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

    pub fn goals(&self) -> &[Goal] {
        &self.goals
    }

    pub fn active_goal(&self) -> Option<&Goal> {
        let identity = self.active_goal.as_ref()?;
        self.goals.iter().find(|goal| &goal.identity == identity)
    }

    pub fn commands(&self) -> &[CommandSpec] {
        &self.commands
    }

    pub fn active_view(&self) -> &str {
        &self.active_view
    }

    pub fn navigate_view(&mut self, forward: bool) -> NorthResult<()> {
        let handler = if forward {
            self.next_view_handler.clone()
        } else {
            self.previous_view_handler.clone()
        };
        self.transition(handler.as_bytes(), &[])
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }

    pub fn host_effect(&self) -> Option<HostEffect> {
        (!self.host_effect.is_empty()).then(|| HostEffect {
            action: self.host_effect.clone(),
            payload: self.effect_payload.clone(),
        })
    }

    pub fn execute_command(&mut self, command: &str) -> NorthResult<()> {
        let handler = self
            .commands
            .iter()
            .find(|candidate| candidate.name == command)
            .map(|candidate| candidate.handler.clone())
            .ok_or_else(|| NorthError::Protocol(format!("Unknown command: {command}")))?;
        self.transition(handler.as_bytes(), &[text_argument("command", command)?])
    }

    pub fn submit_input(&mut self, input: &str) -> NorthResult<()> {
        let handler = self.input_handler.clone();
        self.transition(handler.as_bytes(), &[text_argument("input", input)?])
    }

    pub fn clear_host_effect(&mut self) -> NorthResult<()> {
        self.transition(b"clear-host-effect", &[])?;
        if self.host_effect.is_empty() && self.effect_payload.is_empty() {
            Ok(())
        } else {
            Err(NorthError::Protocol(
                "Clause retained a settled host effect".into(),
            ))
        }
    }

    pub fn create_goal(&mut self, title: &str, objective: &str) -> NorthResult<()> {
        let previous_count = self.goals.len();
        self.execute_command("/goal")?;
        self.submit_input(title)?;
        self.submit_input(objective)?;
        if self.goals.len() != previous_count + 1
            || self.active_goal().is_none_or(|goal| {
                goal.title() != title || goal.objective() != objective || goal.status() != "active"
            })
        {
            return Err(NorthError::Protocol(
                "Clause did not create and activate the requested Goal".into(),
            ));
        }
        Ok(())
    }

    pub fn select_goal(&mut self, index: usize) -> NorthResult<()> {
        let identity = self
            .goals
            .get(index)
            .map(|goal| goal.identity.clone())
            .ok_or_else(|| NorthError::Protocol(format!("Goal index {index} is out of range")))?;
        self.transition(
            b"select-goal",
            &[ExecutableValueV1::Referent(identity.clone())],
        )?;
        if self.active_goal.as_ref() != Some(&identity) {
            return Err(NorthError::Protocol(
                "Clause did not select the requested Goal".into(),
            ));
        }
        Ok(())
    }

    pub fn redirect_active_goal(&mut self, objective: &str) -> NorthResult<()> {
        let previous = self
            .active_goal()
            .map(|goal| goal.objective.clone())
            .ok_or_else(|| NorthError::Protocol("No active Goal to redirect".into()))?;
        self.execute_command("/redirect")?;
        self.submit_input(objective)?;
        let active = self.active_goal().ok_or_else(|| {
            NorthError::Protocol("Clause lost the active Goal during redirect".into())
        })?;
        if active.objective() != objective
            || !active
                .prior_objectives()
                .iter()
                .any(|prior| prior == &previous)
        {
            return Err(NorthError::Protocol(
                "Clause did not redirect the active Goal with immutable history".into(),
            ));
        }
        Ok(())
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
        self.goals = projection.goals;
        self.active_goal = projection.active_goal;
        self.commands = projection.commands;
        self.input_handler = projection.input_handler;
        self.host_effect = projection.host_effect;
        self.effect_payload = projection.effect_payload;
        self.notice = projection.notice;
        self.active_view = projection.active_view;
        self.next_view_handler = projection.next_view_handler;
        self.previous_view_handler = projection.previous_view_handler;
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
    goals: Vec<Goal>,
    active_goal: Option<ExecutableReferentV1>,
    commands: Vec<CommandSpec>,
    input_handler: String,
    host_effect: String,
    effect_payload: String,
    notice: String,
    active_view: String,
    next_view_handler: String,
    previous_view_handler: String,
}

fn decode_projection(exact_term_bytes: &[u8]) -> NorthResult<NorthProjection> {
    let term = decode_canonical_term_bytes(exact_term_bytes).map_err(|error| {
        NorthError::Protocol(format!("conversation state did not decode: {error}"))
    })?;
    let north = projected_object_field(&term, b"north-main")?;
    let phase = projected_text(projected_object_field(north, b"phase")?)?;
    let phase = match phase {
        "idle" => Ok(NorthPhase::Idle),
        "dispatching" => Ok(NorthPhase::Dispatching),
        "delegating" => Ok(NorthPhase::Delegating),
        "settling" => Ok(NorthPhase::Settling),
        "completed" => Ok(NorthPhase::Completed),
        "failed" => Ok(NorthPhase::Failed),
        other => Err(NorthError::Protocol(format!(
            "conversation state projected unknown North phase {}",
            other
        ))),
    }?;
    let conversation_change =
        projected_text(projected_object_field(north, b"conversation-change")?)?;
    let conversation_change = match conversation_change {
        "conversation-ready" => Ok(ConversationChange::Ready),
        "conversation-opening" => Ok(ConversationChange::Opening),
        "conversation-switching" => Ok(ConversationChange::Switching),
        other => Err(NorthError::Protocol(format!(
            "conversation state projected unknown conversation change {}",
            other
        ))),
    }?;
    let relations = projected_object_field(&term, b"relations")?;
    let (goals, active_goal) = projected_goals(relations)?;
    let commands = projected_commands(relations)?;
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
        goals,
        active_goal,
        commands,
        input_handler: relation_single_text(relations, b"input-handler")?,
        host_effect: relation_single_text(relations, b"host-effect")?,
        effect_payload: relation_single_text(relations, b"effect-payload")?,
        notice: relation_single_text(relations, b"notice")?,
        active_view: relation_single_text(relations, b"active-view")?,
        next_view_handler: relation_single_text(relations, b"next-view-handler")?,
        previous_view_handler: relation_single_text(relations, b"previous-view-handler")?,
    })
}

fn child_argument(child_id: &str) -> NorthResult<ExecutableValueV1> {
    identity_argument("worker id", child_id)
}

fn conversation_argument(conversation_id: &str) -> NorthResult<ExecutableValueV1> {
    identity_argument("conversation id", conversation_id)
}

fn identity_argument(label: &str, identity: &str) -> NorthResult<ExecutableValueV1> {
    text_argument(label, identity)
}

fn text_argument(label: &str, value: &str) -> NorthResult<ExecutableValueV1> {
    ExecutableValueV1::text(value).map_err(|error| {
        NorthError::Protocol(format!(
            "{label} cannot be represented in conversation state: {error}"
        ))
    })
}

fn projected_child_identity(term: &Term) -> NorthResult<Option<String>> {
    let identity = projected_text(term)?;
    if identity.is_empty() {
        return Ok(None);
    }
    Ok(Some(identity.to_owned()))
}

fn projected_conversation_identity(term: &Term) -> NorthResult<Option<String>> {
    let identity = projected_text(term)?;
    if identity.is_empty() {
        return Ok(None);
    }
    Ok(Some(identity.to_owned()))
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

fn projected_text(term: &Term) -> NorthResult<&str> {
    projected_text_value_v1(term)
        .map_err(|error| {
            NorthError::Protocol(format!(
                "conversation state projected invalid Text: {error}"
            ))
        })?
        .ok_or_else(|| NorthError::Protocol("conversation state projected a non-Text value".into()))
}

fn projected_goals(relations: &Term) -> NorthResult<(Vec<Goal>, Option<ExecutableReferentV1>)> {
    let known = projected_relation(relations, b"known-goal")?;
    let active = projected_relation(relations, b"active-goal")?;
    let titles = projected_relation(relations, b"goal-title")?;
    let objectives = projected_relation(relations, b"goal-objective")?;
    let statuses = projected_relation(relations, b"goal-status")?;
    let orders = projected_relation(relations, b"goal-order")?;
    let known_revisions = projected_relation(relations, b"known-goal-revision")?;
    let revision_objectives = projected_relation(relations, b"revision-objective")?;
    let revision_orders = projected_relation(relations, b"revision-order")?;

    let identities = known
        .rows()
        .values()
        .flat_map(|values| values.iter())
        .map(|value| {
            value.as_referent().cloned().ok_or_else(|| {
                NorthError::Protocol("known-goal projected a non-Referent value".into())
            })
        })
        .collect::<NorthResult<Vec<_>>>()?;
    let active_goal = active
        .rows()
        .values()
        .flat_map(|values| values.iter())
        .filter_map(ExecutableValueV1::as_referent)
        .find(|candidate| identities.iter().any(|known| known == *candidate))
        .cloned();

    let mut goals = identities
        .into_iter()
        .map(|identity| {
            let title = relation_text(&titles, &identity, "goal-title")?;
            let objective = relation_text(&objectives, &identity, "goal-objective")?;
            let status = relation_text(&statuses, &identity, "goal-status")?;
            let order = relation_integer(&orders, &identity, "goal-order")?;
            let mut revisions = known_revisions
                .rows()
                .get(&identity)
                .into_iter()
                .flatten()
                .map(|value| {
                    let revision = value.as_referent().ok_or_else(|| {
                        NorthError::Protocol(
                            "known-goal-revision projected a non-Referent value".into(),
                        )
                    })?;
                    Ok((
                        relation_integer(&revision_orders, revision, "revision-order")?,
                        relation_text(&revision_objectives, revision, "revision-objective")?,
                    ))
                })
                .collect::<NorthResult<Vec<_>>>()?;
            revisions.sort_by_key(|(order, _)| *order);
            Ok(Goal {
                identity,
                title,
                objective,
                status,
                order,
                prior_objectives: revisions
                    .into_iter()
                    .map(|(_, objective)| objective)
                    .collect(),
            })
        })
        .collect::<NorthResult<Vec<_>>>()?;
    goals.sort_by_key(Goal::order);
    Ok((goals, active_goal))
}

fn projected_commands(relations: &Term) -> NorthResult<Vec<CommandSpec>> {
    let known = projected_relation(relations, b"known-command")?;
    let names = projected_relation(relations, b"command-name")?;
    let descriptions = projected_relation(relations, b"command-description")?;
    let handlers = projected_relation(relations, b"command-handler")?;
    let orders = projected_relation(relations, b"command-order")?;
    let mut commands = known
        .rows()
        .values()
        .flat_map(|values| values.iter())
        .map(|value| {
            let identity = value.as_referent().ok_or_else(|| {
                NorthError::Protocol("known-command projected a non-Referent value".into())
            })?;
            Ok(CommandSpec {
                name: relation_text(&names, identity, "command-name")?,
                description: relation_text(&descriptions, identity, "command-description")?,
                handler: relation_text(&handlers, identity, "command-handler")?,
                order: relation_integer(&orders, identity, "command-order")?,
            })
        })
        .collect::<NorthResult<Vec<_>>>()?;
    commands.sort_by_key(|command| command.order);
    Ok(commands)
}

fn relation_single_text(relations: &Term, designation: &[u8]) -> NorthResult<String> {
    let relation = projected_relation(relations, designation)?;
    let mut values = relation.rows().values().flat_map(|values| values.iter());
    let value = values.next().ok_or_else(|| {
        NorthError::Protocol(format!(
            "{} projected no value",
            String::from_utf8_lossy(designation)
        ))
    })?;
    if values.next().is_some() {
        return Err(NorthError::Protocol(format!(
            "{} projected more than one value",
            String::from_utf8_lossy(designation)
        )));
    }
    value.as_text().map(str::to_owned).ok_or_else(|| {
        NorthError::Protocol(format!(
            "{} projected a non-Text value",
            String::from_utf8_lossy(designation)
        ))
    })
}

fn projected_relation(
    relations: &Term,
    designation: &[u8],
) -> NorthResult<ExecutableRelationTableV1> {
    let term = projected_object_field(relations, designation)?;
    projected_relation_table_v1(term)
        .map_err(|error| {
            NorthError::Protocol(format!(
                "{} projected an invalid relation table: {error}",
                String::from_utf8_lossy(designation)
            ))
        })?
        .ok_or_else(|| {
            NorthError::Protocol(format!(
                "{} did not project a relation table",
                String::from_utf8_lossy(designation)
            ))
        })
}

fn relation_value<'a>(
    table: &'a ExecutableRelationTableV1,
    subject: &ExecutableReferentV1,
    label: &str,
) -> NorthResult<&'a ExecutableValueV1> {
    let values = table
        .rows()
        .get(subject)
        .ok_or_else(|| NorthError::Protocol(format!("{label} lacks its Goal row")))?;
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Err(NorthError::Protocol(format!(
            "{label} did not project exactly one value"
        )));
    };
    if values.next().is_some() {
        return Err(NorthError::Protocol(format!(
            "{label} did not project exactly one value"
        )));
    }
    Ok(value)
}

fn relation_text(
    table: &ExecutableRelationTableV1,
    subject: &ExecutableReferentV1,
    label: &str,
) -> NorthResult<String> {
    relation_value(table, subject, label)?
        .as_text()
        .map(str::to_owned)
        .ok_or_else(|| NorthError::Protocol(format!("{label} projected a non-Text value")))
}

fn relation_integer(
    table: &ExecutableRelationTableV1,
    subject: &ExecutableReferentV1,
    label: &str,
) -> NorthResult<u64> {
    let value = relation_value(table, subject, label)?
        .as_number()
        .ok_or_else(|| NorthError::Protocol(format!("{label} projected a non-numeric value")))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || !(1.0..=(MAX_EXACT_F64_INTEGER as f64)).contains(&value)
    {
        return Err(NorthError::Protocol(format!(
            "{label} projected invalid positive integer {value}"
        )));
    }
    Ok(value as u64)
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
    fn clause_creates_orders_selects_and_redirects_goals_with_revisions() {
        let mut state = NorthState::open().expect("North Clause source opens");
        assert!(state.goals().is_empty());
        assert!(state.active_goal().is_none());

        state
            .create_goal("Build North", "Make Clause own Goals")
            .expect("first Goal is created");
        state
            .create_goal("Ship North", "Run the real TUI journey")
            .expect("second Goal is created");
        assert_eq!(
            state
                .goals()
                .iter()
                .map(|goal| (goal.order(), goal.title()))
                .collect::<Vec<_>>(),
            vec![(1, "Build North"), (2, "Ship North")]
        );
        assert_eq!(state.active_goal().map(Goal::title), Some("Ship North"));

        state.select_goal(0).expect("first Goal is selected");
        state
            .redirect_active_goal("Make Clause own ordered Goal revisions")
            .expect("first redirect is admitted");
        state
            .redirect_active_goal("Make Clause own all Goal semantics")
            .expect("repeated redirect is admitted");
        let active = state.active_goal().expect("selected Goal remains active");
        assert_eq!(active.title(), "Build North");
        assert_eq!(active.objective(), "Make Clause own all Goal semantics");
        assert_eq!(
            active.prior_objectives(),
            [
                "Make Clause own Goals",
                "Make Clause own ordered Goal revisions"
            ]
        );
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
