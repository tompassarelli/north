use clause_package::{Term, decode_canonical_term_bytes};
use clause_runtime::{
    ExecutableReferentV1, ExecutableRelationTableV1, ExecutableValueV1,
    projected_relation_table_v1, projected_text_value_v1, projected_referent_value_v1,
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
    Interrupted,
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
pub struct ViewSpec {
    pub name: String,
    pub label: String,
    order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatEntry {
    pub visible: bool,
    pub conversation: String,
    pub turn: String,
    pub key: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub style: String,
    order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingInput {
    pub number: u64,
    pub conversation: String,
    pub text: String,
    pub status: String,
    pub attachments: Vec<AttachmentIdentity>,
}

#[derive(Clone, Debug)]
pub struct ConversationState {
    pub transcript_query: String,
    pub transcript_offset: u64,
    pub transcript_limit: u64,
    pub transcript_changes: bool,
    pub id: String,
    pub model: String,
    pub effort: String,
    pub attached: bool,
    pub phase: NorthPhase,
    pub active_turn: String,
    pub active_child: Option<String>,
    pub terminal_child: Option<String>,
    pub draft_attachments: Vec<AttachmentIdentity>,
    pub submitted_attachments: Vec<AttachmentIdentity>,
    pub saved_draft: String,
}

#[derive(Clone)]
pub struct Prompt {
    pub number: u64,
    pub request: String,
    pub conversation: String,
    pub kind: String,
    pub cancellation: String,
    pub outcome: String,
    pub status: String,
    pub visible: bool,
    pub current: u64,
    pub questions: Vec<PromptQuestion>,
}

#[derive(Clone)]
pub struct PromptQuestion {
    pub codec: String,
    pub multiple: bool,
    pub optional: bool,
    pub problem: String,
    pub number: u64,
    pub key: String,
    pub header: String,
    pub text: String,
    pub secret: bool,
    pub other: bool,
    pub text_entry: bool,
    pub selected: u64,
    pub answer: String,
    pub answered: bool,
    pub options: Vec<PromptOption>,
}

#[derive(Clone)]
pub struct PromptOption {
    pub value: String,
    pub checked: bool,
    pub number: u64,
    pub label: String,
    pub description: String,
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
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

pub struct NorthState {
    workbench: ResidentSourceWorkbenchV1,
    revision: Option<clause_package::StateRevisionId>,
    references: Vec<crate::references::Reference>,
    reference_query: String,
    reference_selection: usize,
    references_open: bool,
    contexts: Vec<ConversationState>,
    chat: Vec<ChatEntry>,
    pending_inputs: Vec<PendingInput>,
    prompts: Vec<Prompt>,
    next_prompt_number: u64,
    active_turn: String,
    effect_input_number: u64,
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
    views: Vec<ViewSpec>,
    input_handler: String,
    input_dispatch: String,
    input_payload: String,
    input_kind: String,
    host_effect: String,
    effect_payload: String,
    notice: String,
    active_view: String,
    next_view_handler: String,
    previous_view_handler: String,
}

impl NorthState {
    pub fn open() -> NorthResult<Self> {
        let workbench = ResidentSourceWorkbenchV1::open_continuous(NORTH_SOURCE)?;
        let mut state = Self {
            revision: None,
            workbench,
            references: Vec::new(),
            reference_query: String::new(),
            reference_selection: 0,
            references_open: false,
            contexts: Vec::new(),
            chat: Vec::new(),
            pending_inputs: Vec::new(),
            prompts: Vec::new(),
            next_prompt_number: 1,
            active_turn: String::new(),
            effect_input_number: 1,
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
            views: Vec::new(),
            input_handler: "submit-input".into(),
            input_dispatch: String::new(),
            input_payload: String::new(),
            input_kind: "message".into(),
            host_effect: String::new(),
            effect_payload: String::new(),
            notice: String::new(),
            active_view: "chat".into(),
            next_view_handler: "view-chat-next".into(),
            previous_view_handler: "view-chat-previous".into(),
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

    pub const fn revision(&self) -> Option<clause_package::StateRevisionId> {
        self.revision
    }

    pub fn chat(&self) -> &[ChatEntry] {
        &self.chat
    }

    pub fn references(&self) -> &[crate::references::Reference] { &self.references }
    pub fn reference_selection(&self) -> usize { self.reference_selection }
    pub fn reference_query(&self) -> &str { &self.reference_query }
    pub fn references_open(&self) -> bool { self.references_open }

    pub fn query_references(&mut self, query: &str, candidates: &[crate::references::Reference]) -> NorthResult<()> {
        let mut transitions: Vec<(&[u8], Vec<ExecutableValueV1>)> = vec![
            (b"clear-references", vec![]),
            (b"query-references", vec![text_argument("query", query)?]),
        ];
        for candidate in candidates {
            let path = candidate.path.to_str().ok_or_else(|| NorthError::Protocol("A reference filename cannot be represented as text.".into()))?;
            transitions.push((b"offer-reference", [&candidate.name, &candidate.description, &candidate.kind, path]
                .into_iter().map(|text| text_argument("reference", text)).collect::<NorthResult<_>>()?));
        }
        self.transition_sequence(&transitions)
    }

    pub fn move_reference(&mut self, delta: isize) -> NorthResult<()> {
        self.transition(b"move-reference", &[ExecutableValueV1::number(delta as f64).map_err(|error| NorthError::State(error.to_string()))?])
    }

    pub fn dismiss_references(&mut self) -> NorthResult<()> {
        self.transition(b"dismiss-references", &[])
    }

    pub fn active_turn(&self) -> &str {
        &self.active_turn
    }

    pub fn observe_turn(&mut self, conversation: &str, turn: &str) -> NorthResult<()> {
        self.transition_sequence(&[
            (b"ensure-context", vec![conversation_argument(conversation)?]),
            (b"observe-turn", vec![conversation_argument(conversation)?, text_argument("turn", turn)?]),
        ])
    }

    pub fn observe_stored_turn(&mut self, conversation: &str, turn: &str, status: &str) -> NorthResult<()> {
        self.text_transition(b"observe-stored-turn", &[conversation, turn, status])
    }

    pub fn conversation(&self, id: &str) -> Option<&ConversationState> {
        self.contexts.iter().find(|context| context.id == id)
    }

    pub fn save_draft(&mut self, text: &str) -> NorthResult<()> {
        self.text_transition(b"save-draft", &[text])
    }

    pub fn search_transcript(&mut self, query: &str) -> NorthResult<()> {
        self.text_transition(b"search-transcript", &[query])
    }

    pub fn scroll_transcript(&mut self, delta: f64) -> NorthResult<()> {
        self.transition(b"scroll-transcript", &[ExecutableValueV1::number(delta).map_err(|error| NorthError::State(error.to_string()))?])
    }

    pub fn size_transcript(&mut self, limit: u64) -> NorthResult<()> {
        self.transition(b"size-transcript", &[AttachmentIdentity(limit).argument()?])
    }

    pub fn toggle_changes(&mut self) -> NorthResult<()> {
        self.transition(b"toggle-changes", &[])
    }

    pub fn observe_settings(&mut self, id: &str, model: &str, effort: &str) -> NorthResult<()> {
        self.text_transition(b"observe-settings", &[id, model, effort])
    }

    pub fn finish_observed_turn(&mut self, conversation: &str, turn: &str) -> NorthResult<()> {
        let arguments = vec![text_argument("conversation", conversation)?, text_argument("turn", turn)?];
        self.transition_sequence(&[
            (b"finish-observed-turn", arguments.clone()),
            (b"close-turn-prompts", arguments),
        ])
    }

    pub fn pending_inputs(&self) -> &[PendingInput] {
        &self.pending_inputs
    }

    pub fn prompts(&self) -> &[Prompt] { &self.prompts }

    pub fn active_prompt(&self) -> Option<&Prompt> {
        self.prompts.iter().find(|prompt| prompt.visible)
    }

    pub fn observe_prompt(&mut self, input: &crate::prompts::IncomingPrompt) -> NorthResult<()> {
        let number = self.next_prompt_number;
        let mut steps = vec![(b"observe-prompt".as_slice(), [&input.request, &input.conversation, &input.turn, &input.kind, &input.cancellation]
            .map(|value| text_argument("prompt", value)).into_iter().collect::<NorthResult<Vec<_>>>()?)];
        for (index, question) in input.questions.iter().enumerate() {
            let position = index as u64 + 1;
            steps.push((b"observe-prompt-question".as_slice(), vec![
                AttachmentIdentity(number).argument()?, AttachmentIdentity(position).argument()?,
                text_argument("key", &question.key)?, text_argument("header", &question.header)?,
                text_argument("question", &question.text)?, ExecutableValueV1::Boolean(question.secret),
                ExecutableValueV1::Boolean(question.other),
                text_argument("codec", &question.codec)?, ExecutableValueV1::Boolean(question.multiple),
                ExecutableValueV1::Boolean(question.optional),
            ]));
            for (index, option) in question.options.iter().enumerate() {
                steps.push((b"observe-prompt-option".as_slice(), vec![
                    AttachmentIdentity(number).argument()?, AttachmentIdentity(position).argument()?,
                    AttachmentIdentity(index as u64 + 1).argument()?, text_argument("label", &option.label)?,
                    text_argument("description", &option.description)?, text_argument("value", &option.value)?,
                ]));
            }
        }
        self.transition_sequence(&steps)
    }

    pub fn navigate_prompt(&mut self, direction: i32) -> NorthResult<()> {
        self.transition(b"navigate-prompt", &[ExecutableValueV1::Number((direction as f64).to_bits())])
    }

    pub fn answer_prompt_choice(&mut self) -> NorthResult<()> {
        self.transition(b"answer-prompt-choice", &[])
    }

    pub fn answer_prompt_text(&mut self, text: &str) -> NorthResult<()> {
        self.text_transition(b"answer-prompt-text", &[text])
    }

    pub fn toggle_prompt_option(&mut self) -> NorthResult<()> {
        self.transition(b"toggle-prompt-option", &[])
    }

    pub fn answer_prompt_multiple(&mut self, text: &str) -> NorthResult<()> {
        self.text_transition(b"answer-prompt-multiple", &[text])
    }

    pub fn skip_prompt_question(&mut self) -> NorthResult<()> {
        self.transition(b"skip-prompt-question", &[])
    }

    pub fn prompt_answer_problem(&mut self, text: &str) -> NorthResult<()> {
        self.text_transition(b"prompt-answer-problem", &[text])
    }

    pub fn cancel_prompt(&mut self) -> NorthResult<()> {
        self.transition(b"cancel-prompt", &[])
    }

    pub fn prepare_prompt_response(&mut self) -> NorthResult<Option<u64>> {
        self.transition(b"prepare-prompt-response", &[])?;
        Ok((self.host_effect == "answer-prompt").then_some(self.effect_input_number))
    }

    pub fn prompt_response_written(&mut self, number: u64, success: bool) -> NorthResult<()> {
        self.transition(b"prompt-response-written", &[
            AttachmentIdentity(number).argument()?,
            text_argument("result", if success { "waiting for confirmation" } else { "delivery unknown" })?,
        ])
    }

    pub fn resolve_prompt(&mut self, conversation: &str, request: &str) -> NorthResult<()> {
        self.text_transition(b"resolve-prompt", &[conversation, request])
    }

    pub fn queue_input(&mut self, text: &str) -> NorthResult<u64> {
        self.retain_input(text, "queue")
    }

    pub fn retain_steering(&mut self, text: &str) -> NorthResult<u64> {
        self.retain_input(text, "steer")
    }

    pub fn input_receipt(&mut self, number: u64, result: &str) -> NorthResult<()> {
        let arguments = vec![
            AttachmentIdentity(number).argument()?,
            text_argument("receipt", result)?,
        ];
        self.transition_sequence(&[
            (b"record-input-acceptance", arguments.clone()),
            (b"input-receipt", arguments),
        ])
    }

    pub fn prepare_input_edit(&mut self) -> NorthResult<Option<u64>> {
        self.transition(b"prepare-input-edit", &[])?;
        Ok((self.host_effect == "edit-pending").then_some(self.effect_input_number))
    }

    pub fn restore_input(&mut self, number: u64) -> NorthResult<()> {
        self.remove_input(number, true)
    }

    pub fn forget_input(&mut self, number: u64) -> NorthResult<()> {
        self.remove_input(number, false)
    }

    fn remove_input(&mut self, number: u64, restore: bool) -> NorthResult<()> {
        let input = self.pending_inputs.iter().find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Pending input is missing".into()))?;
        let mut steps = Vec::new();
        for attachment in &input.attachments {
            let args = vec![AttachmentIdentity(number).argument()?, attachment.argument()?];
            if restore {
                steps.push((b"restore-input-attachment".as_slice(), args.clone()));
            }
            steps.push((b"forget-input-attachment".as_slice(), args));
        }
        steps.push((b"forget-input".as_slice(), vec![AttachmentIdentity(number).argument()?]));
        self.transition_sequence(&steps)?;
        if self.pending_inputs.iter().any(|input| input.number == number) {
            return Err(NorthError::Protocol("Pending input is not ready for removal".into()));
        }
        Ok(())
    }

    fn retain_input(&mut self, text: &str, mode: &str) -> NorthResult<u64> {
        let number = self.draft_number;
        let mut steps = vec![(b"retain-input".as_slice(), vec![text_argument("input", text)?, text_argument("mode", mode)?])];
        for attachment in &self.draft_attachments {
            steps.push((b"retain-queued-attachment".as_slice(), vec![
                AttachmentIdentity(number).argument()?, attachment.argument()?,
            ]));
        }
        self.transition_sequence(&steps)?;
        Ok(number)
    }

    pub fn prepare_queued_input(&mut self) -> NorthResult<Option<u64>> {
        self.transition(b"prepare-queued-input", &[])?;
        Ok((self.host_effect == "submit-queued").then_some(self.effect_input_number))
    }

    pub fn prepare_steering(&mut self) -> NorthResult<Option<u64>> {
        self.transition(b"prepare-steering", &[])?;
        Ok((self.host_effect == "send-steering").then_some(self.effect_input_number))
    }

    pub fn submit_queued(&mut self, number: u64) -> NorthResult<Vec<AttachmentIdentity>> {
        let input = self.pending_inputs.iter().find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Queued input is missing".into()))?;
        let attachments = input.attachments.clone();
        let conversation = input.conversation.clone();
        let number = AttachmentIdentity(number).argument()?;
        let mut steps = Vec::new();
        for attachment in &attachments {
            steps.push((b"copy-queued-to-submitted".as_slice(), vec![number.clone(), attachment.argument()?]));
        }
        steps.push((b"submit-queued".as_slice(), vec![number]));
        self.transition_sequence(&steps)?;
        if self.conversation(&conversation).is_none_or(|context| context.phase != NorthPhase::Dispatching) {
            return Err(NorthError::State("Queued input did not start its conversation".into()));
        }
        Ok(attachments)
    }

    pub fn append_chat(&mut self, kind: &str, text: &str) -> NorthResult<()> {
        let conversation = self.active_conversation().unwrap_or_default().to_owned();
        self.append_chat_in(&conversation, kind, text)
    }

    pub fn append_chat_in(&mut self, conversation: &str, kind: &str, text: &str) -> NorthResult<()> {
        self.text_transition(b"append-chat", &[conversation, kind, text])
    }

    pub fn observe_chat_item(&mut self, item: &ChatEntryInput<'_>) -> NorthResult<()> {
        self.text_transition(b"observe-chat-item", &[
            item.conversation, item.turn, item.key, item.kind, item.text,
            item.status, if item.append { "append" } else { "replace" },
        ])
    }

    pub fn clear_chat(&mut self) -> NorthResult<()> {
        self.transition(b"clear-chat", &[])
    }

    fn text_transition(&mut self, event: &[u8], fields: &[&str]) -> NorthResult<()> {
        let values = fields.iter().map(|text| ExecutableValueV1::text(text)
            .map_err(|error| NorthError::Protocol(format!("Invalid event text: {error}"))))
            .collect::<NorthResult<Vec<_>>>()?;
        self.transition(event, &values)
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

    pub fn views(&self) -> &[ViewSpec] {
        &self.views
    }

    pub fn show_chat(&mut self) -> NorthResult<()> {
        self.transition(b"show-chat", &[])
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
        self.accept_input(command)
    }

    pub fn submit_input(&mut self, input: &str) -> NorthResult<()> {
        self.accept_input(input)
    }

    pub fn input_is_command(&self) -> bool {
        self.input_kind == "command"
    }

    pub fn accept_input(&mut self, input: &str) -> NorthResult<()> {
        self.transition(b"resolve-input", &[text_argument("input", input)?])?;
        if self.input_dispatch.is_empty() {
            return Ok(());
        }
        let handler = self.input_dispatch.clone();
        let payload = self.input_payload.clone();
        self.transition(handler.as_bytes(), &[text_argument("input", &payload)?])
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

    pub fn edit_active_goal(&mut self, objective: &str) -> NorthResult<()> {
        let previous = self
            .active_goal()
            .map(|goal| goal.objective.clone())
            .ok_or_else(|| NorthError::Protocol("No selected goal to edit".into()))?;
        self.execute_command("/goal edit")?;
        self.clear_host_effect()?;
        self.submit_input(objective)?;
        let active = self.active_goal().ok_or_else(|| {
            NorthError::Protocol("Clause lost the active Goal during edit".into())
        })?;
        if active.objective() != objective
            || !active
                .prior_objectives()
                .iter()
                .any(|prior| prior == &previous)
        {
            return Err(NorthError::Protocol(
                "Clause did not edit the active Goal with immutable history".into(),
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
        self.transition_sequence(&[
            (b"ensure-context", vec![conversation.clone()]),
            (b"settle-new-conversation", vec![conversation]),
        ])?;
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
        self.transition_sequence(&[
            (b"ensure-context", vec![conversation.clone()]),
            (b"settle-switch-conversation", vec![conversation]),
        ])?;
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
        let conversation = conversation_argument(self.active_conversation().unwrap_or_default())?;
        self.transition(b"child-spawned", &[conversation, child])?;
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
        let conversation = self.active_conversation().unwrap_or_default().to_owned();
        self.finish_turn_in(&conversation, designation, arguments)
    }

    fn finish_turn_in(
        &mut self,
        conversation: &str,
        designation: &'static [u8],
        arguments: &[ExecutableValueV1],
    ) -> NorthResult<()> {
        let context = self.conversation(conversation).ok_or_else(|| NorthError::State("Turn has no conversation".into()))?;
        let attachments = context.submitted_attachments.clone();
        let conversation_value = conversation_argument(conversation)?;
        let mut transitions = Vec::with_capacity(attachments.len() + 1);
        for identity in attachments {
            transitions.push((
                b"clear-submitted-attachment".as_slice(),
                vec![conversation_value.clone(), identity.argument()?],
            ));
        }
        let mut target_arguments = vec![conversation_value.clone()];
        target_arguments.extend_from_slice(arguments);
        transitions.push((designation, target_arguments));
        transitions.push((b"expire-steering".as_slice(), vec![conversation_value.clone()]));
        transitions.push((b"clear-turn".as_slice(), vec![conversation_value]));
        self.transition_sequence(&transitions)?;
        if !self.conversation(conversation).unwrap().submitted_attachments.is_empty() {
            return Err(NorthError::Protocol(
                "Clause retained attachments after turn settlement".into(),
            ));
        }
        Ok(())
    }

    pub fn settle_direct_in(&mut self, conversation: &str, succeeded: bool) -> NorthResult<()> {
        let context = self.conversation(conversation).ok_or_else(|| NorthError::State("Turn has no conversation".into()))?;
        if context.phase != NorthPhase::Dispatching {
            return Err(NorthError::State("Conversation has no direct turn to settle".into()));
        }
        self.finish_turn_in(conversation, if succeeded { b"settle-success" } else { b"settle-failure" }, &[])
    }

    pub fn abandon_turn_in(&mut self, conversation: &str) -> NorthResult<()> {
        self.finish_turn_in(conversation, b"abandon-turn", &[])
    }

    pub fn settle_interrupted_in(&mut self, conversation: &str, child: Option<&str>) -> NorthResult<()> {
        if let Some(child) = child { self.text_transition(b"child-spawned", &[conversation, child])?; }
        self.finish_turn_in(conversation, b"settle-interrupted", &[])
    }

    pub fn settle_delegation_in(&mut self, conversation: &str, child: Option<&str>, succeeded: bool) -> NorthResult<()> {
        if let Some(child) = child {
            self.text_transition(b"child-spawned", &[conversation, child])?;
            self.finish_turn_in(conversation, if succeeded { b"settle-delegation-success" } else { b"fail-delegation-after-child" }, &[child_argument(child)?])
        } else {
            self.finish_turn_in(conversation, b"fail-delegation-before-child", &[])
        }
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
        let mut occurrences = transitions
            .iter()
            .map(|(designation, arguments)| {
                self.workbench.handler_occurrence(designation, arguments)
            })
            .collect::<Result<Vec<_>, _>>()?;
        occurrences.push(self.workbench.handler_occurrence(b"clear-resolved-answers", &[])?);
        occurrences.push(self.workbench.handler_occurrence(b"focus-prompts", &[])?);
        occurrences.push(self.workbench.handler_occurrence(b"present-prompt-questions", &[])?);
        occurrences.push(self.workbench.handler_occurrence(b"filter-transcript", &[])?);
        self.workbench.run_occurrences_to_candidate(&occurrences)?;
        let admission = self.workbench.admit()?;
        let projection = decode_projection(&admission.projection.exact_term_bytes)?;
        self.revision = Some(admission.successor);
        self.references = projection.references;
        self.reference_query = projection.reference_query;
        self.reference_selection = projection.reference_selection;
        self.references_open = projection.references_open;
        self.chat = projection.chat;
        self.contexts = projection.contexts;
        self.pending_inputs = projection.pending_inputs;
        self.prompts = projection.prompts;
        self.next_prompt_number = projection.next_prompt_number;
        self.active_turn = projection.active_turn;
        self.effect_input_number = projection.effect_input_number;
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
        self.views = projection.views;
        self.input_handler = projection.input_handler;
        self.input_dispatch = projection.input_dispatch;
        self.input_payload = projection.input_payload;
        self.input_kind = projection.input_kind;
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
    references: Vec<crate::references::Reference>,
    reference_query: String,
    reference_selection: usize,
    references_open: bool,
    contexts: Vec<ConversationState>,
    chat: Vec<ChatEntry>,
    pending_inputs: Vec<PendingInput>,
    prompts: Vec<Prompt>,
    next_prompt_number: u64,
    active_turn: String,
    effect_input_number: u64,
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
    views: Vec<ViewSpec>,
    input_handler: String,
    input_dispatch: String,
    input_payload: String,
    input_kind: String,
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
    let views = projected_views(&term, relations)?;
    let contexts = projected_contexts(relations)?;
    let active_id = projected_text(projected_object_field(north, b"active-conversation")?)?;
    let active = contexts.iter().find(|context| context.id == active_id)
        .ok_or_else(|| NorthError::State(format!("Selected conversation {active_id:?} is missing")))?;
    Ok(NorthProjection {
        references: projected_references(relations)?,
        reference_query: relation_single_text(relations, b"reference-query")?,
        reference_selection: relation_single_natural(relations, b"reference-selection")? as usize,
        references_open: relation_single_boolean(relations, b"references-open")?,
        chat: projected_chat(relations)?,
        pending_inputs: projected_pending_inputs(relations)?,
        prompts: projected_prompts(relations)?,
        next_prompt_number: projected_integer(projected_object_field(north, b"next-prompt-number")?)?,
        active_turn: active.active_turn.clone(),
        effect_input_number: relation_single_integer(relations, b"effect-input-number")?,
        phase: active.phase,
        active_delegated_child: active.active_child.clone(),
        terminal_delegated_child: active.terminal_child.clone(),
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
        draft_attachments: active.draft_attachments.clone(),
        submitted_attachments: active.submitted_attachments.clone(),
        contexts,
        goals,
        active_goal,
        commands,
        views,
        input_handler: relation_single_text(relations, b"input-handler")?,
        input_dispatch: relation_single_text(relations, b"input-dispatch")?,
        input_payload: relation_single_text(relations, b"input-payload")?,
        input_kind: relation_single_text(relations, b"input-kind")?,
        host_effect: relation_single_text(relations, b"host-effect")?,
        effect_payload: relation_single_text(relations, b"effect-payload")?,
        notice: relation_single_text(relations, b"notice")?,
        active_view: relation_single_text(relations, b"active-view")?,
        next_view_handler: relation_single_text(relations, b"next-view-handler")?,
        previous_view_handler: relation_single_text(relations, b"previous-view-handler")?,
    })
}

fn projected_contexts(relations: &Term) -> NorthResult<Vec<ConversationState>> {
    let ids = projected_relation(relations, b"conversation-id")?;
    let phases = projected_relation(relations, b"phase")?;
    let turns = projected_relation(relations, b"active-turn")?;
    let children = projected_relation(relations, b"active-delegated-child")?;
    let terminals = projected_relation(relations, b"terminal-delegated-child")?;
    let drafts = projected_relation(relations, b"draft-attachment")?;
    let submissions = projected_relation(relations, b"submitted-attachment")?;
    let saved = projected_relation(relations, b"saved-draft")?;
    let queries = projected_relation(relations, b"transcript-query")?;
    let offsets = projected_relation(relations, b"transcript-offset")?;
    let limits = projected_relation(relations, b"transcript-limit")?;
    let changes = projected_relation(relations, b"transcript-changes")?;
    let models = projected_relation(relations, b"context-model")?;
    let efforts = projected_relation(relations, b"context-effort")?;
    let attached = projected_relation(relations, b"context-attached")?;
    let child = |table, subject, label| -> NorthResult<Option<String>> {
        let text = relation_text(table, subject, label)?;
        Ok((!text.is_empty()).then_some(text))
    };
    let attachments = |table: &ExecutableRelationTableV1, subject: &ExecutableReferentV1| -> NorthResult<Vec<AttachmentIdentity>> {
        let mut values = table.rows().get(subject).into_iter().flatten().map(attachment_value).collect::<NorthResult<Vec<_>>>()?;
        values.sort();
        Ok(values)
    };
    ids.rows().keys().map(|subject| {
        let phase = match relation_text(&phases, subject, "phase")?.as_str() {
            "idle" => NorthPhase::Idle,
            "dispatching" => NorthPhase::Dispatching,
            "delegating" => NorthPhase::Delegating,
            "settling" => NorthPhase::Settling,
            "completed" => NorthPhase::Completed,
            "interrupted" => NorthPhase::Interrupted,
            "failed" => NorthPhase::Failed,
            other => return Err(NorthError::State(format!("Unknown conversation phase {other:?}"))),
        };
        Ok(ConversationState {
            transcript_query: relation_text(&queries, subject, "transcript-query")?,
            transcript_offset: relation_natural(&offsets, subject, "transcript-offset")?,
            transcript_limit: relation_natural(&limits, subject, "transcript-limit")?,
            transcript_changes: relation_boolean(&changes, subject, "transcript-changes")?,
            id: relation_text(&ids, subject, "conversation-id")?, phase,
            model: relation_text(&models, subject, "context-model")?,
            effort: relation_text(&efforts, subject, "context-effort")?,
            attached: relation_boolean(&attached, subject, "context-attached")?,
            active_turn: relation_text(&turns, subject, "active-turn")?,
            active_child: child(&children, subject, "active-delegated-child")?,
            terminal_child: child(&terminals, subject, "terminal-delegated-child")?,
            draft_attachments: attachments(&drafts, subject)?,
            submitted_attachments: attachments(&submissions, subject)?,
            saved_draft: relation_text(&saved, subject, "saved-draft")?,
        })
    }).collect()
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

pub struct ChatEntryInput<'a> {
    pub conversation: &'a str,
    pub turn: &'a str,
    pub key: &'a str,
    pub kind: &'a str,
    pub text: &'a str,
    pub status: &'a str,
    pub append: bool,
}

fn relation_boolean(table: &ExecutableRelationTableV1, subject: &ExecutableReferentV1, label: &str) -> NorthResult<bool> {
    match relation_value(table, subject, label)? {
        ExecutableValueV1::Boolean(value) => Ok(*value),
        _ => Err(NorthError::Protocol(format!("{label} projected a non-Boolean value"))),
    }
}

fn relation_natural(table: &ExecutableRelationTableV1, subject: &ExecutableReferentV1, label: &str) -> NorthResult<u64> {
    let value = relation_value(table, subject, label)?.as_number()
        .ok_or_else(|| NorthError::Protocol(format!("{label} projected a non-numeric value")))?;
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=(MAX_EXACT_F64_INTEGER as f64)).contains(&value) {
        return Err(NorthError::Protocol(format!("{label} projected an invalid index")));
    }
    Ok(value as u64)
}

fn projected_prompts(relations: &Term) -> NorthResult<Vec<Prompt>> {
    let known = projected_relation(relations, b"known-prompt")?;
    let questions = projected_relation(relations, b"prompt-question")?;
    let options = projected_relation(relations, b"question-option")?;
    let prompt_number = projected_relation(relations, b"prompt-number")?;
    let prompt_request = projected_relation(relations, b"prompt-request")?;
    let prompt_conversation = projected_relation(relations, b"prompt-conversation")?;
    let prompt_kind = projected_relation(relations, b"prompt-kind")?;
    let prompt_cancellation = projected_relation(relations, b"prompt-cancellation")?;
    let prompt_outcome = projected_relation(relations, b"prompt-outcome")?;
    let prompt_status = projected_relation(relations, b"prompt-status")?;
    let prompt_visible = projected_relation(relations, b"prompt-visible")?;
    let prompt_current = projected_relation(relations, b"prompt-current")?;
    let question_number = projected_relation(relations, b"question-number")?;
    let question_key = projected_relation(relations, b"question-key")?;
    let question_header = projected_relation(relations, b"question-header")?;
    let question_text = projected_relation(relations, b"question-text")?;
    let question_codec = projected_relation(relations, b"question-codec")?;
    let question_multiple = projected_relation(relations, b"question-multiple")?;
    let question_optional = projected_relation(relations, b"question-optional")?;
    let question_problem = projected_relation(relations, b"question-problem")?;
    let question_secret = projected_relation(relations, b"question-secret")?;
    let question_other = projected_relation(relations, b"question-other")?;
    let question_text_entry = projected_relation(relations, b"question-text-entry")?;
    let question_selected = projected_relation(relations, b"question-selected")?;
    let question_answer = projected_relation(relations, b"question-answer")?;
    let question_answered = projected_relation(relations, b"question-answered")?;
    let option_number = projected_relation(relations, b"option-number")?;
    let option_label = projected_relation(relations, b"option-label")?;
    let option_description = projected_relation(relations, b"option-description")?;
    let option_value = projected_relation(relations, b"option-value")?;
    let option_checked = projected_relation(relations, b"option-checked")?;
    let mut result = Vec::new();
    for value in known.rows().values().flatten() {
        let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("Prompt lacks identity".into()))?;
        let mut prompt_questions = Vec::new();
        for value in questions.rows().get(identity).into_iter().flatten() {
            let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("Question lacks identity".into()))?;
            let mut question_options = Vec::new();
            for value in options.rows().get(identity).into_iter().flatten() {
                let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("Option lacks identity".into()))?;
                question_options.push(PromptOption {
                    value: relation_text(&option_value, identity, "option-value")?,
                    checked: relation_boolean(&option_checked, identity, "option-checked")?,
                    number: relation_integer(&option_number, identity, "option-number")?,
                    label: relation_text(&option_label, identity, "option-label")?,
                    description: relation_text(&option_description, identity, "option-description")?,
                });
            }
            question_options.sort_by_key(|option| option.number);
            prompt_questions.push(PromptQuestion {
                codec: relation_text(&question_codec, identity, "question-codec")?,
                multiple: relation_boolean(&question_multiple, identity, "question-multiple")?,
                optional: relation_boolean(&question_optional, identity, "question-optional")?,
                problem: relation_text(&question_problem, identity, "question-problem")?,
                text_entry: relation_boolean(&question_text_entry, identity, "question-text-entry")?,
                number: relation_integer(&question_number, identity, "question-number")?,
                    key: relation_text(&question_key, identity, "question-key")?,
                    header: relation_text(&question_header, identity, "question-header")?,
                    text: relation_text(&question_text, identity, "question-text")?,
                    answer: relation_text(&question_answer, identity, "question-answer")?,
                    secret: relation_boolean(&question_secret, identity, "question-secret")?,
                    other: relation_boolean(&question_other, identity, "question-other")?,
                    answered: relation_boolean(&question_answered, identity, "question-answered")?,
                    selected: relation_natural(&question_selected, identity, "question-selected")?,
                options: question_options,
            });
        }
        prompt_questions.sort_by_key(|question| question.number);
        result.push(Prompt {
            number: relation_integer(&prompt_number, identity, "prompt-number")?,
                    request: relation_text(&prompt_request, identity, "prompt-request")?,
                    conversation: relation_text(&prompt_conversation, identity, "prompt-conversation")?,
                    kind: relation_text(&prompt_kind, identity, "prompt-kind")?,
                    cancellation: relation_text(&prompt_cancellation, identity, "prompt-cancellation")?,
                    outcome: relation_text(&prompt_outcome, identity, "prompt-outcome")?,
                    status: relation_text(&prompt_status, identity, "prompt-status")?,
                    visible: relation_boolean(&prompt_visible, identity, "prompt-visible")?,
                    current: relation_integer(&prompt_current, identity, "prompt-current")?,
            questions: prompt_questions,
        });
    }
    result.sort_by_key(|prompt| prompt.number);
    Ok(result)
}

fn projected_pending_inputs(relations: &Term) -> NorthResult<Vec<PendingInput>> {
    let known = projected_relation(relations, b"known-pending-input")?;
    let number = projected_relation(relations, b"pending-input-number")?;
    let conversation = projected_relation(relations, b"pending-input-conversation")?;
    let text = projected_relation(relations, b"pending-input-text")?;
    let status = projected_relation(relations, b"pending-input-status")?;
    let attachments = projected_relation(relations, b"pending-input-attachment")?;
    let mut inputs = known.rows().values().flatten().map(|value| {
        let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("Input lacks identity".into()))?;
        Ok(PendingInput {
            number: relation_integer(&number, identity, "pending-input-number")?,
            conversation: relation_text(&conversation, identity, "pending-input-conversation")?,
            text: relation_text(&text, identity, "pending-input-text")?,
            status: relation_text(&status, identity, "pending-input-status")?,
            attachments: attachments.rows().get(identity).into_iter().flatten()
                .map(attachment_value).collect::<NorthResult<Vec<_>>>()?,
        })
    }).collect::<NorthResult<Vec<_>>>()?;
    inputs.sort_by_key(|input| input.number);
    Ok(inputs)
}

fn relation_single_integer(relations: &Term, name: &[u8]) -> NorthResult<u64> {
    let table = projected_relation(relations, name)?;
    let mut subjects = table.rows().keys();
    let subject = subjects.next().ok_or_else(|| NorthError::Protocol("Missing numeric state".into()))?;
    if subjects.next().is_some() { return Err(NorthError::Protocol("Ambiguous numeric state".into())); }
    relation_integer(&table, subject, &String::from_utf8_lossy(name))
}

fn projected_references(relations: &Term) -> NorthResult<Vec<crate::references::Reference>> {
    let known = projected_relation(relations, b"known-reference")?;
    let names = projected_relation(relations, b"reference-name")?;
    let descriptions = projected_relation(relations, b"reference-description")?;
    let kinds = projected_relation(relations, b"reference-kind")?;
    let paths = projected_relation(relations, b"reference-path")?;
    let numbers = projected_relation(relations, b"reference-number")?;
    let mut references = known.rows().values().flatten().map(|value| {
        let id = value.as_referent().ok_or_else(|| NorthError::State("Reference lacks identity".into()))?;
        Ok((relation_natural(&numbers, id, "reference-number")?, crate::references::Reference {
            name: relation_text(&names, id, "reference-name")?,
            description: relation_text(&descriptions, id, "reference-description")?,
            kind: relation_text(&kinds, id, "reference-kind")?,
            path: relation_text(&paths, id, "reference-path")?.into(),
        }))
    }).collect::<NorthResult<Vec<_>>>()?;
    references.sort_by_key(|(number, _)| *number);
    Ok(references.into_iter().map(|(_, reference)| reference).collect())
}

fn projected_chat(relations: &Term) -> NorthResult<Vec<ChatEntry>> {
    let known = projected_relation(relations, b"known-chat-entry")?;
    let conversation = projected_relation(relations, b"chat-conversation")?;
    let turn = projected_relation(relations, b"chat-turn")?;
    let key = projected_relation(relations, b"chat-key")?;
    let kind = projected_relation(relations, b"chat-kind")?;
    let text = projected_relation(relations, b"chat-text")?;
    let status = projected_relation(relations, b"chat-status")?;
    let style = projected_relation(relations, b"chat-style")?;
    let order = projected_relation(relations, b"chat-order")?;
    let visible = projected_relation(relations, b"chat-visible")?;
    let mut entries = known.rows().values().flatten().map(|value| {
        let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("Chat entry lacks identity".into()))?;
        Ok(ChatEntry {
            visible: relation_boolean(&visible, identity, "chat-visible")?,
            conversation: relation_text(&conversation, identity, "chat-conversation")?,
            turn: relation_text(&turn, identity, "chat-turn")?,
            key: relation_text(&key, identity, "chat-key")?,
            kind: relation_text(&kind, identity, "chat-kind")?,
            text: relation_text(&text, identity, "chat-text")?,
            status: relation_text(&status, identity, "chat-status")?,
            style: relation_text(&style, identity, "chat-style")?,
            order: relation_integer(&order, identity, "chat-order")?,
        })
    }).collect::<NorthResult<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.order);
    Ok(entries)
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

fn projected_views(frame: &Term, relations: &Term) -> NorthResult<Vec<ViewSpec>> {
    let known = projected_relation(relations, b"known-view")?;
    let mut views = known.rows().values().flat_map(|values| values.iter())
        .map(|value| {
            let identity = value.as_referent().ok_or_else(|| NorthError::Protocol("known-view projected a non-Referent value".into()))?;
            let view = projected_declared_subject(frame, identity)?;
            Ok(ViewSpec {
                name: projected_text(projected_object_field(view, b"view-name")?)?.to_owned(),
                label: projected_text(projected_object_field(view, b"view-label")?)?.to_owned(),
                order: projected_integer(projected_object_field(view, b"view-order")?)?,
            })
        }).collect::<NorthResult<Vec<_>>>()?;
    views.sort_by_key(|view| view.order);
    Ok(views)
}

fn projected_declared_subject<'a>(frame: &'a Term, identity: &ExecutableReferentV1) -> NorthResult<&'a Term> {
    let mut current = frame;
    while let Some(triple) = current.as_triple() {
        let [_, subject, rest] = triple.slots();
        if let Ok(reference) = projected_object_field(subject, b"$referent") {
            let reference = projected_referent_value_v1(reference)
                .map_err(|error| NorthError::Protocol(format!("invalid projected referent: {error}")))?;
            if reference.as_ref() == Some(identity) {
                return Ok(subject);
            }
        }
        current = rest;
    }
    Err(NorthError::Protocol("projection lacks the declared subject".into()))
}

fn projected_commands(relations: &Term) -> NorthResult<Vec<CommandSpec>> {
    let known = projected_relation(relations, b"known-command")?;
    let names = projected_relation(relations, b"command-name")?;
    let descriptions = projected_relation(relations, b"command-description")?;
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
                order: relation_integer(&orders, identity, "command-order")?,
            })
        })
        .collect::<NorthResult<Vec<_>>>()?;
    commands.sort_by_key(|command| command.order);
    Ok(commands)
}

fn relation_single_boolean(relations: &Term, designation: &[u8]) -> NorthResult<bool> {
    let relation = projected_relation(relations, designation)?;
    let mut values = relation.rows().values().flatten();
    match (values.next(), values.next()) {
        (Some(ExecutableValueV1::Boolean(value)), None) => Ok(*value),
        _ => Err(NorthError::State(format!("{} must project exactly one Boolean value", String::from_utf8_lossy(designation)))),
    }
}

fn relation_single_natural(relations: &Term, designation: &[u8]) -> NorthResult<u64> {
    let relation = projected_relation(relations, designation)?;
    let mut subjects = relation.rows().keys();
    match (subjects.next(), subjects.next()) {
        (Some(subject), None) => relation_natural(&relation, subject, &String::from_utf8_lossy(designation)),
        _ => Err(NorthError::State(format!("{} must project exactly one row", String::from_utf8_lossy(designation)))),
    }
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
        .ok_or_else(|| NorthError::State(format!(
            "relation {label:?} has no row for subject {subject:?}; this view requires exactly one value"
        )))?;
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Err(NorthError::State(format!(
            "relation {label:?} has an empty row for subject {subject:?}; this view requires exactly one value"
        )));
    };
    if values.next().is_some() {
        return Err(NorthError::State(format!(
            "relation {label:?} has multiple values for subject {subject:?}; this view requires exactly one value"
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

fn attachment_value(value: &ExecutableValueV1) -> NorthResult<AttachmentIdentity> {
        let number = value.as_number().ok_or_else(|| NorthError::Protocol("Attachment is not numeric".into()))?;
        if !number.is_finite() || number.fract() != 0.0 || !(1.0..=MAX_EXACT_F64_INTEGER as f64).contains(&number) {
            return Err(NorthError::Protocol("Invalid attachment identity".into()));
        }
        Ok(AttachmentIdentity(number as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_catalog_can_be_replaced_after_a_full_listing() {
        let mut state = NorthState::open().unwrap();
        let candidates = (0..256).map(|number| crate::references::Reference {
            name: format!("src/file-{number}.rs"),
            description: "Project file".into(),
            kind: "file".into(),
            path: format!("/project/src/file-{number}.rs").into(),
        }).collect::<Vec<_>>();
        state.query_references("", &candidates).unwrap();
        assert_eq!(state.references().len(), 256);
        state.query_references("file-255", &candidates).unwrap();
        assert_eq!(state.references().len(), 1);
        assert_eq!(state.references()[0].name, "src/file-255.rs");
    }

    #[test]
    fn conversation_turns_queues_and_drafts_are_independent_of_selection() {
        let mut state = NorthState::open().unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("alpha").unwrap();
        state.submit().unwrap();
        state.observe_turn("alpha", "alpha-turn").unwrap();
        let alpha_image = state.attach_image().unwrap();
        state.save_draft("alpha draft").unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("beta").unwrap();
        assert_eq!(state.phase(), NorthPhase::Idle);
        assert_eq!(state.active_turn(), "");
        assert!(state.draft_attachments.is_empty());
        state.submit().unwrap();
        state.observe_turn("beta", "beta-turn").unwrap();
        let queued = state.queue_input("beta follow-up").unwrap();
        let beta_image = state.attach_image().unwrap();
        state.save_draft("beta draft").unwrap();
        state.request_switch_conversation("alpha").unwrap();
        state.settle_switch_conversation("alpha").unwrap();
        assert_eq!(state.phase(), NorthPhase::Dispatching);
        assert_eq!(state.active_turn(), "alpha-turn");
        assert_eq!(state.draft_attachments, vec![alpha_image]);
        assert_eq!(state.conversation("alpha").unwrap().saved_draft, "alpha draft");
        state.finish_observed_turn("beta", "beta-turn").unwrap();
        state.settle_direct_in("beta", true).unwrap();
        assert_eq!(state.phase(), NorthPhase::Dispatching);
        assert_eq!(state.active_turn(), "alpha-turn");
        assert_eq!(state.prepare_queued_input().unwrap(), Some(queued));
        state.clear_host_effect().unwrap();
        state.submit_queued(queued).unwrap();
        state.settle_direct_in("alpha", false).unwrap();
        assert_eq!(state.conversation("beta").unwrap().phase, NorthPhase::Dispatching);
        assert_eq!(state.conversation("beta").unwrap().draft_attachments, vec![beta_image]);
        assert_eq!(state.conversation("beta").unwrap().saved_draft, "beta draft");
    }

    #[test]
    fn incomplete_rows_identify_the_relation_and_subject_without_inventing_a_goal() {
        let mut workbench = ResidentSourceWorkbenchV1::open(NORTH_SOURCE).unwrap();
        let occurrence = workbench.handler_occurrence(b"initialize", &[]).unwrap();
        workbench.run_occurrences_to_candidate(&[occurrence]).unwrap();
        let term = decode_canonical_term_bytes(&workbench.admit().unwrap().projection.exact_term_bytes).unwrap();
        let table = projected_relation(projected_object_field(&term, b"relations").unwrap(), b"chat-text").unwrap();
        let subject = ExecutableReferentV1::declared(table.subject_domain(), u32::MAX);
        let error = relation_value(&table, &subject, "chat-text").unwrap_err();
        assert!(error.to_string().contains("relation \"chat-text\" has no row for subject"));
        assert!(!error.to_string().contains("Goal"));
        assert_eq!(error.user_message(), "North couldn’t read the conversation state.");
    }

    #[test]
    fn queued_inputs_wait_for_settlement_and_keep_their_own_attachments() {
        let mut state = NorthState::open().unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("thread").unwrap();
        state.submit().unwrap();
        let image = state.attach_image().unwrap();
        let first = state.queue_input("first follow-up").unwrap();
        let second = state.queue_input("second follow-up").unwrap();
        let next_draft_image = state.attach_image().unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), None);
        state.settle_success().unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), Some(first));
        state.clear_host_effect().unwrap();
        assert_eq!(state.submit_queued(first).unwrap(), vec![image]);
        state.settle_success().unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), Some(second));
        state.clear_host_effect().unwrap();
        assert_eq!(state.submit_queued(second).unwrap(), vec![]);
        state.settle_success().unwrap();
        assert_eq!(state.submit().unwrap(), vec![next_draft_image]);
    }

    #[test]
    fn running_input_becomes_steering_and_failed_delivery_stays_unsent() {
        let mut state = NorthState::open().unwrap();
        state.submit().unwrap();
        state.accept_input("focus on the UI").unwrap();
        assert_eq!(state.host_effect().unwrap().action(), "steer");
        state.clear_host_effect().unwrap();
        let id = state.retain_steering("focus on the UI").unwrap();
        state.observe_turn("", "turn").unwrap();
        assert_eq!(state.prepare_steering().unwrap(), Some(id));
        state.clear_host_effect().unwrap();
        state.input_receipt(id, "not sent").unwrap();
        state.settle_success().unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), None);
        assert_eq!(state.pending_inputs()[0].status, "not sent");
        assert_eq!(state.pending_inputs()[0].text, "focus on the UI");
    }

    #[test]
    fn steering_waits_for_receipts_and_expiry_never_retargets_a_new_turn() {
        let mut state = NorthState::open().unwrap();
        state.submit().unwrap();
        let first = state.retain_steering("first correction").unwrap();
        let second = state.retain_steering("second correction").unwrap();
        assert_eq!(state.prepare_steering().unwrap(), None);
        state.observe_turn("", "turn-first").unwrap();
        assert_eq!(state.prepare_steering().unwrap(), Some(first));
        state.clear_host_effect().unwrap();
        assert_eq!(state.prepare_steering().unwrap(), None);
        state.finish_observed_turn("", "turn-unrelated").unwrap();
        assert_eq!(state.active_turn(), "turn-first");
        state.settle_success().unwrap();
        assert!(state.active_turn().is_empty());
        assert_eq!(state.pending_inputs()[1].status, "not sent");
        state.input_receipt(first, "accepted").unwrap();
        state.input_receipt(first, "accepted").unwrap();
        assert_eq!(state.chat().len(), 1);
        assert_eq!(state.chat()[0].text, "first correction");
        state.forget_input(first).unwrap();
        state.submit().unwrap();
        state.observe_turn("", "turn-next").unwrap();
        assert_eq!(state.prepare_steering().unwrap(), None);
        assert_eq!(state.pending_inputs()[0].number, second);
    }

    #[test]
    fn unknown_delivery_is_retained_without_allowing_an_unreconciled_retry() {
        let mut state = NorthState::open().unwrap();
        state.submit().unwrap();
        state.observe_turn("", "turn-first").unwrap();
        let input = state.retain_steering("do not duplicate this").unwrap();
        state.prepare_steering().unwrap();
        state.clear_host_effect().unwrap();
        state.input_receipt(input, "delivery unknown").unwrap();
        state.settle_failure().unwrap();
        assert_eq!(state.prepare_input_edit().unwrap(), None);
        assert_eq!(state.prepare_queued_input().unwrap(), None);
        assert_eq!(state.pending_inputs()[0].status, "delivery unknown");
        assert_eq!(state.pending_inputs()[0].text, "do not duplicate this");
    }

    #[test]
    fn streamed_items_keep_order_and_replace_completed_text_without_duplicates() {
        let mut state = NorthState::open().unwrap();
        state.append_chat("operator", "show progress").unwrap();
        let mut item = ChatEntryInput {
            conversation: "thread", turn: "turn", key: "message", kind: "agentMessage",
            text: "Working", status: "inProgress", append: true,
        };
        state.observe_chat_item(&item).unwrap();
        item.text = " now";
        state.observe_chat_item(&item).unwrap();
        assert_eq!(state.chat()[1].text, "Working now");
        item.text = "Working now.";
        item.status = "completed";
        item.append = false;
        state.observe_chat_item(&item).unwrap();
        assert_eq!(state.chat().len(), 2);
        assert_eq!(state.chat()[1].text, "Working now.");
        item.conversation = "other-thread";
        state.observe_chat_item(&item).unwrap();
        assert_eq!(state.chat().len(), 3);
    }

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
    fn clause_creates_orders_selects_and_edits_goals_with_revisions() {
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
            .edit_active_goal("Make Clause own ordered Goal revisions")
            .expect("first edit is admitted");
        state
            .edit_active_goal("Make Clause own all Goal semantics")
            .expect("repeated edit is admitted");
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
