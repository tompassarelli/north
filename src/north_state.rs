pub use crate::menu::Menu as MenuState;
pub use crate::state_core::{
    AttachmentIdentity, ChatEntry, CommandSpec, ConversationChange, ConversationState, Goal,
    NorthPhase, PendingInput, Prompt, PromptOption, PromptQuestion, UsagePanel, UsageRow, ViewSpec,
};
use crate::state_core::{Event, State};
pub type StateRevisionId = u64;
use crate::error::{NorthError, NorthResult};
const MAX_EXACT_F64_INTEGER: u64 = (1 << 53) - 1;
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

pub struct NorthState {
    store: Option<crate::local_store::LocalStore>,
    storage_failed: bool,
    checkpoint_depth: usize,
    connection_state: String,
    menu: MenuState,
    usage: UsagePanel,
    workbench: State,
    revision: Option<StateRevisionId>,
    references: Vec<crate::references::Reference>,
    reference_query: String,
    reference_selection: usize,
    references_open: bool,
    contexts: Vec<ConversationState>,
    chat: std::sync::Arc<Vec<ChatEntry>>,
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
    active_goal: Option<u64>,
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
        Self::open_stored(None)
    }

    pub(crate) fn open_stored(store: Option<crate::local_store::LocalStore>) -> NorthResult<Self> {
        Self::open_backend(store)
    }

    fn open_backend(mut store: Option<crate::local_store::LocalStore>) -> NorthResult<Self> {
        let checkpoint = store
            .as_mut()
            .map(|store| store.read())
            .transpose()?
            .flatten();
        let workbench = State::open(checkpoint.as_deref())?;
        let mut state = Self {
            store,
            storage_failed: false,
            checkpoint_depth: 0,
            connection_state: "connected".into(),
            menu: menu_view(&workbench.menu),
            usage: UsagePanel::default(),
            revision: None,
            workbench,
            references: Vec::new(),
            reference_query: String::new(),
            reference_selection: 0,
            references_open: false,
            contexts: Vec::new(),
            chat: Default::default(),
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
        let reopened = checkpoint.is_some();
        if reopened {
            state.checkpoint_after(|state| {
                state.connection_lost()?;
                state.show_chat()
            })?;
        } else {
            state.transition(Event::Initialize)?;
        }
        if !reopened && state.phase != NorthPhase::Idle {
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

    pub fn connection_state(&self) -> &str {
        &self.connection_state
    }

    pub fn attached_conversations(&self) -> impl Iterator<Item = &str> {
        self.contexts
            .iter()
            .filter(|context| context.attached && !context.id.is_empty())
            .map(|context| context.id.as_str())
    }

    pub fn connection_lost(&mut self) -> NorthResult<()> {
        self.transition_sequence(&[
            Event::ConnectionLost,
            Event::PauseDisconnectedInputs,
            Event::DisconnectPrompts,
        ])
    }

    pub fn begin_reconnect(&mut self) -> NorthResult<()> {
        self.transition(Event::BeginReconnect)
    }

    pub fn finish_reconnect(&mut self, success: bool) -> NorthResult<()> {
        self.transition(Event::FinishReconnect { success })
    }

    pub fn begin_conversation_reconciliation(&mut self, conversation: &str) -> NorthResult<()> {
        self.transition(Event::BeginConversationReconciliation {
            conversation: conversation.into(),
        })
    }

    pub const fn revision(&self) -> Option<StateRevisionId> {
        self.revision
    }

    pub fn chat(&self) -> &[ChatEntry] {
        &self.chat
    }

    pub fn references(&self) -> &[crate::references::Reference] {
        &self.references
    }
    pub fn menu(&self) -> &MenuState {
        &self.menu
    }
    pub fn usage(&self) -> &UsagePanel {
        &self.usage
    }
    pub fn compaction_completed(&mut self) -> NorthResult<()> {
        self.transition(Event::CompactionCompleted)
    }

    pub fn observe_context_usage(
        &mut self,
        update: &crate::usage::ContextUpdate,
    ) -> NorthResult<()> {
        let usage = &update.token_usage;
        self.transition_sequence(&[
            Event::WithdrawUsageRow {
                owner: (&update.thread_id).to_owned(),
                key: ("context").to_owned(),
            },
            Event::WithdrawUsageRow {
                owner: (&update.thread_id).to_owned(),
                key: ("tokens").to_owned(),
            },
            Event::ObserveContextUsage {
                conversation: (&update.thread_id).to_owned(),
                used: numeric_argument(usage.last.total_tokens)?,
                window: numeric_argument(usage.model_context_window.unwrap_or_default())?,
                total: numeric_argument(usage.total.total_tokens)?,
            },
        ])
    }

    pub fn observe_account_usage(
        &mut self,
        usage: &crate::usage::AccountUsage,
        now: u64,
    ) -> NorthResult<()> {
        let mut steps = Vec::new();
        let buckets = match usage.rate_limits_by_limit_id.as_ref() {
            Some(buckets) => buckets
                .iter()
                .map(|(id, bucket)| (id.as_str(), bucket))
                .collect::<Vec<_>>(),
            None => vec![(
                usage.rate_limits.limit_id.as_deref().unwrap_or("codex"),
                &usage.rate_limits,
            )],
        };
        for (id, bucket) in buckets {
            for (slot, window) in [
                ("primary", &bucket.primary),
                ("secondary", &bucket.secondary),
            ] {
                let key = format!("{id}/{slot}");
                steps.push(Event::WithdrawUsageRow {
                    owner: ("").to_owned(),
                    key: (&key).to_owned(),
                });
                if let Some(window) = window {
                    steps.push(Event::ObserveQuotaWindow {
                        key: (&key).to_owned(),
                        name: (bucket.limit_name.as_deref().unwrap_or(id)).to_owned(),
                        used: numeric_argument(window.used_percent)?,
                        duration: numeric_argument(
                            window.window_duration_mins.unwrap_or_default(),
                        )?,
                        reset: numeric_argument(window.resets_at.unwrap_or_default())?,
                        now: numeric_argument(now)?,
                    });
                }
            }
        }
        steps.push(Event::WithdrawUsageRow {
            owner: ("").to_owned(),
            key: ("reset-credits").to_owned(),
        });
        steps.push(Event::ObserveResetCredits {
            count: numeric_argument(
                usage
                    .rate_limit_reset_credits
                    .as_ref()
                    .map_or(0, |credits| credits.available_count),
            )?,
            known: usage.rate_limit_reset_credits.is_some(),
        });
        self.transition_sequence(&steps)
    }

    pub fn observe_archived(&mut self, conversation: &str) -> NorthResult<()> {
        self.transition(Event::ObserveArchived {
            conversation: conversation.into(),
        })
    }

    pub fn observe_restored(&mut self, conversation: &str) -> NorthResult<()> {
        self.transition(Event::ObserveRestored {
            conversation: conversation.into(),
        })
    }

    pub fn history_action_completed(&mut self, action: &str) -> NorthResult<()> {
        self.transition(Event::HistoryActionCompleted {
            action: action.to_owned(),
        })
    }
    pub fn reference_selection(&self) -> usize {
        self.reference_selection
    }
    pub fn reference_query(&self) -> &str {
        &self.reference_query
    }
    pub fn references_open(&self) -> bool {
        self.references_open
    }

    pub fn query_references(
        &mut self,
        query: &str,
        candidates: &[crate::references::Reference],
    ) -> NorthResult<()> {
        let mut transitions: Vec<Event> = vec![
            Event::ClearReferences,
            Event::QueryReferences {
                query: (query).to_owned(),
            },
        ];
        for candidate in candidates {
            let path = candidate.path.to_str().ok_or_else(|| {
                NorthError::Protocol("A reference filename cannot be represented as text.".into())
            })?;
            transitions.push(Event::OfferReference {
                name: candidate.name.clone(),
                description: candidate.description.clone(),
                kind: candidate.kind.clone(),
                path: path.into(),
            });
        }
        self.transition_sequence(&transitions)
    }

    pub fn move_reference(&mut self, delta: isize) -> NorthResult<()> {
        self.transition(Event::MoveReference {
            delta: exact_integer(delta as f64)?,
        })
    }

    pub fn dismiss_references(&mut self) -> NorthResult<()> {
        self.transition(Event::DismissReferences)
    }

    pub fn active_turn(&self) -> &str {
        &self.active_turn
    }

    pub fn observe_turn(&mut self, conversation: &str, turn: &str) -> NorthResult<()> {
        self.transition_sequence(&[
            Event::EnsureContext {
                conversation: (conversation).to_owned(),
            },
            Event::ObserveTurn {
                conversation: (conversation).to_owned(),
                turn: (turn).to_owned(),
            },
        ])
    }

    pub fn conversation(&self, id: &str) -> Option<&ConversationState> {
        self.contexts.iter().find(|context| context.id == id)
    }

    pub(crate) fn conversations(&self) -> &[ConversationState] {
        &self.contexts
    }

    pub(crate) fn retain_image_file(&self, path: &std::path::Path) -> NorthResult<()> {
        if let Some(store) = &self.store {
            store.save_image(self.next_attachment_number, path)?;
        }
        Ok(())
    }

    pub(crate) fn image_file(
        &self,
        identity: AttachmentIdentity,
    ) -> NorthResult<tempfile::NamedTempFile> {
        self.store
            .as_ref()
            .ok_or_else(|| NorthError::State("Saved image storage is unavailable".into()))?
            .load_image(identity.number())
    }

    pub fn save_draft(&mut self, text: &str) -> NorthResult<()> {
        self.transition(Event::SaveDraft {
            text: text.to_owned(),
        })
    }

    pub fn search_transcript(&mut self, query: &str) -> NorthResult<()> {
        self.transition(Event::SearchTranscript {
            query: query.to_owned(),
        })
    }

    pub fn scroll_transcript(&mut self, delta: f64) -> NorthResult<()> {
        self.transition(Event::ScrollTranscript {
            delta: exact_integer(delta)?,
        })
    }

    pub fn size_transcript(&mut self, limit: u64) -> NorthResult<()> {
        self.transition(Event::SizeTranscript {
            limit: attachment_number(limit)?,
        })
    }

    pub fn toggle_changes(&mut self) -> NorthResult<()> {
        self.transition(Event::ToggleChanges)
    }

    pub fn observe_settings(&mut self, id: &str, model: &str, effort: &str) -> NorthResult<()> {
        self.transition(Event::ObserveSettings {
            conversation: id.to_owned(),
            model: model.to_owned(),
            effort: effort.to_owned(),
        })
    }

    pub fn finish_observed_turn(&mut self, conversation: &str, turn: &str) -> NorthResult<()> {
        self.transition_sequence(&[
            Event::FinishObservedTurn {
                conversation: conversation.into(),
                turn: turn.into(),
            },
            Event::CloseTurnPrompts {
                conversation: conversation.into(),
                turn: turn.into(),
            },
        ])
    }

    pub fn pending_inputs(&self) -> &[PendingInput] {
        &self.pending_inputs
    }

    pub fn prompts(&self) -> &[Prompt] {
        &self.prompts
    }

    pub fn active_prompt(&self) -> Option<&Prompt> {
        self.prompts.iter().find(|prompt| prompt.visible)
    }

    pub fn observe_prompt(&mut self, input: &crate::prompts::IncomingPrompt) -> NorthResult<()> {
        let number = self.next_prompt_number;
        let mut steps = vec![Event::ObservePrompt {
            request: input.request.clone(),
            conversation: input.conversation.clone(),
            turn: input.turn.clone(),
            kind: input.kind.clone(),
            cancellation: input.cancellation.clone(),
        }];
        for (index, question) in input.questions.iter().enumerate() {
            let position = index as u64 + 1;
            steps.push(Event::ObservePromptQuestion {
                number: attachment_number(number)?,
                position: attachment_number(position)?,
                key: (&question.key).to_owned(),
                header: (&question.header).to_owned(),
                text: (&question.text).to_owned(),
                secret: question.secret,
                other: question.other,
                codec: (&question.codec).to_owned(),
                multiple: question.multiple,
                optional: question.optional,
            });
            for (index, option) in question.options.iter().enumerate() {
                steps.push(Event::ObservePromptOption {
                    number: attachment_number(number)?,
                    position: attachment_number(position)?,
                    index: attachment_number(index as u64 + 1)?,
                    label: (&option.label).to_owned(),
                    description: (&option.description).to_owned(),
                    value: (&option.value).to_owned(),
                });
            }
        }
        self.transition_sequence(&steps)
    }

    pub fn navigate_prompt(&mut self, direction: i32) -> NorthResult<()> {
        self.transition(Event::NavigatePrompt {
            direction: direction as i64,
        })
    }

    pub fn answer_prompt_choice(&mut self) -> NorthResult<()> {
        self.transition(Event::AnswerPromptChoice)
    }

    pub fn answer_prompt_text(&mut self, text: &str) -> NorthResult<()> {
        self.transition(Event::AnswerPromptText {
            text: text.to_owned(),
        })
    }

    pub fn toggle_prompt_option(&mut self) -> NorthResult<()> {
        self.transition(Event::TogglePromptOption)
    }

    pub fn answer_prompt_multiple(&mut self, text: &str) -> NorthResult<()> {
        self.transition(Event::AnswerPromptMultiple {
            text: text.to_owned(),
        })
    }

    pub fn skip_prompt_question(&mut self) -> NorthResult<()> {
        self.transition(Event::SkipPromptQuestion)
    }

    pub fn prompt_answer_problem(&mut self, text: &str) -> NorthResult<()> {
        self.transition(Event::PromptAnswerProblem {
            problem: text.to_owned(),
        })
    }

    pub fn cancel_prompt(&mut self) -> NorthResult<()> {
        self.transition(Event::CancelPrompt)
    }

    pub fn prepare_prompt_response(&mut self) -> NorthResult<Option<u64>> {
        self.transition(Event::PreparePromptResponse)?;
        Ok((self.host_effect == "answer-prompt").then_some(self.effect_input_number))
    }

    pub fn prompt_response_written(&mut self, number: u64, success: bool) -> NorthResult<()> {
        self.transition(Event::PromptResponseWritten {
            number: attachment_number(number)?,
            result: (if success {
                "waiting for confirmation"
            } else {
                "delivery unknown"
            })
            .to_owned(),
        })
    }

    pub fn resolve_prompt(&mut self, conversation: &str, request: &str) -> NorthResult<()> {
        self.transition(Event::ResolvePrompt {
            conversation: conversation.into(),
            request: request.to_owned(),
        })
    }

    pub fn queue_input(&mut self, text: &str) -> NorthResult<u64> {
        self.retain_input(text, "queue")
    }

    pub fn retain_steering(&mut self, text: &str) -> NorthResult<u64> {
        self.retain_input(text, "steer")
    }

    pub fn retain_direct(&mut self, text: &str) -> NorthResult<u64> {
        self.retain_input(text, "direct")
    }

    pub fn bind_input_client_id(&mut self, number: u64, client: &str) -> NorthResult<()> {
        self.transition(Event::BindInputClientId {
            number: numeric_argument(number)?,
            client: (client).to_owned(),
        })
    }

    pub fn observe_input_acceptance(
        &mut self,
        conversation: &str,
        client: &str,
    ) -> NorthResult<()> {
        self.transition(Event::ObserveInputAcceptance {
            conversation: conversation.into(),
            client: client.to_owned(),
        })
    }

    pub fn input_receipt(&mut self, number: u64, result: &str) -> NorthResult<()> {
        let number = attachment_number(number)?;
        self.transition_sequence(&[
            Event::RecordInputAcceptance {
                number,
                result: result.into(),
            },
            Event::InputReceipt {
                number,
                result: result.into(),
            },
        ])
    }

    pub fn prepare_input_edit(&mut self) -> NorthResult<Option<u64>> {
        self.transition(Event::PrepareInputEdit)?;
        Ok((self.host_effect == "edit-pending").then_some(self.effect_input_number))
    }

    pub fn restore_input(&mut self, number: u64) -> NorthResult<()> {
        self.remove_input(number, true)
    }

    pub fn forget_input(&mut self, number: u64) -> NorthResult<()> {
        self.remove_input(number, false)
    }

    fn remove_input(&mut self, number: u64, restore: bool) -> NorthResult<()> {
        let input = self
            .pending_inputs
            .iter()
            .find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Pending input is missing".into()))?;
        let mut steps = Vec::new();
        if restore {
            steps.push(Event::SaveDraft {
                text: (&input.text).to_owned(),
            });
        }
        for attachment in &input.attachments {
            let number = attachment_number(number)?;
            let attachment = attachment_number(attachment.number())?;
            if restore {
                steps.push(Event::RestoreInputAttachment { number, attachment });
            }
            steps.push(Event::ForgetInputAttachment { number, attachment });
        }
        steps.push(Event::ForgetInput {
            number: attachment_number(number)?,
        });
        self.transition_sequence(&steps)?;
        if self
            .pending_inputs
            .iter()
            .any(|input| input.number == number)
        {
            return Err(NorthError::Protocol(
                "Pending input is not ready for removal".into(),
            ));
        }
        Ok(())
    }

    fn retain_input(&mut self, text: &str, mode: &str) -> NorthResult<u64> {
        let number = self.draft_number;
        let mut steps = vec![Event::RetainInput {
            text: (text).to_owned(),
            mode: (mode).to_owned(),
        }];
        steps.push(Event::SaveDraft {
            text: ("").to_owned(),
        });
        for attachment in &self.draft_attachments {
            steps.push(Event::RetainQueuedAttachment {
                number: attachment_number(number)?,
                attachment: attachment_number(attachment.number())?,
            });
        }
        self.transition_sequence(&steps)?;
        Ok(number)
    }

    pub fn prepare_queued_input(&mut self) -> NorthResult<Option<u64>> {
        self.transition(Event::PrepareQueuedInput)?;
        Ok((self.host_effect == "submit-queued").then_some(self.effect_input_number))
    }

    pub fn prepare_steering(&mut self) -> NorthResult<Option<u64>> {
        self.transition(Event::PrepareSteering)?;
        Ok((self.host_effect == "send-steering").then_some(self.effect_input_number))
    }

    pub fn submit_queued(&mut self, number: u64) -> NorthResult<Vec<AttachmentIdentity>> {
        let input = self
            .pending_inputs
            .iter()
            .find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Queued input is missing".into()))?;
        let attachments = input.attachments.clone();
        let conversation = input.conversation.clone();
        let number = attachment_number(number)?;
        let mut steps = Vec::new();
        for attachment in &attachments {
            steps.push(Event::CopyQueuedToSubmitted {
                number,
                attachment: attachment_number(attachment.number())?,
            });
        }
        steps.push(Event::SubmitQueued { number });
        self.transition_sequence(&steps)?;
        if self
            .conversation(&conversation)
            .is_none_or(|context| context.phase != NorthPhase::Dispatching)
        {
            return Err(NorthError::State(
                "Queued input did not start its conversation".into(),
            ));
        }
        Ok(attachments)
    }

    pub fn append_chat(&mut self, kind: &str, text: &str) -> NorthResult<()> {
        let conversation = self.active_conversation().unwrap_or_default().to_owned();
        self.append_chat_in(&conversation, kind, text)
    }

    pub fn append_chat_in(
        &mut self,
        conversation: &str,
        kind: &str,
        text: &str,
    ) -> NorthResult<()> {
        self.transition(Event::AppendChat {
            conversation: conversation.into(),
            kind: kind.to_owned(),
            text: text.to_owned(),
        })
    }

    pub fn observe_chat_item(&mut self, item: &ChatEntryInput<'_>) -> NorthResult<()> {
        self.observe_chat_items(std::slice::from_ref(item))
    }

    pub fn observe_chat_items(&mut self, items: &[ChatEntryInput<'_>]) -> NorthResult<()> {
        let steps = items
            .iter()
            .map(|item| Event::ObserveChatItem {
                conversation: item.conversation.into(),
                turn: item.turn.into(),
                key: item.key.into(),
                kind: item.kind.into(),
                text: item.text.into(),
                status: item.status.into(),
                mode: if item.append { "append" } else { "replace" }.into(),
            })
            .collect::<Vec<_>>();
        if steps.is_empty() {
            return Ok(());
        }
        self.transition_sequence(&steps)
    }

    pub fn observe_snapshot(
        &mut self,
        snapshot: &crate::codex::ConversationSnapshot,
    ) -> NorthResult<()> {
        let mut steps = Vec::new();
        for client in &snapshot.accepted_inputs {
            steps.push(Event::ObserveInputAcceptance {
                conversation: snapshot.id.clone(),
                client: client.clone(),
            });
        }
        steps.push(Event::ObserveSettings {
            conversation: snapshot.id.clone(),
            model: snapshot.model.clone(),
            effort: snapshot.reasoning_effort.clone(),
        });
        let mut observed_keys = std::collections::BTreeSet::new();
        for item in &snapshot.entries {
            if observed_keys.insert(item.key.as_str())
                && self.chat.iter().any(|entry| {
                    entry.conversation == snapshot.id
                        && entry.turn == item.turn
                        && entry.key == item.key
                        && entry.kind == item.kind
                        && entry.text == item.text
                        && entry.status == item.status
                })
            {
                continue;
            }
            steps.push(Event::ObserveChatItem {
                conversation: snapshot.id.clone(),
                turn: item.turn.clone(),
                key: item.key.clone(),
                kind: item.kind.clone(),
                text: item.text.clone(),
                status: item.status.clone(),
                mode: "replace".into(),
            });
        }
        for turn in &snapshot.turns {
            steps.push(Event::ObserveStoredTurn {
                conversation: snapshot.id.clone(),
                turn: turn.id.clone(),
                status: turn.status.clone(),
            });
        }
        self.transition_sequence(&steps)
    }

    pub fn clear_chat(&mut self) -> NorthResult<()> {
        self.transition(Event::ClearChat)
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
        self.transition(Event::ShowChat)
    }

    pub fn active_view(&self) -> &str {
        &self.active_view
    }

    pub fn navigate_view(&mut self, forward: bool) -> NorthResult<()> {
        let event = match (self.active_view.as_str(), forward) {
            ("goals", true) => Event::ViewGoalsNext,
            ("goals", false) => Event::ViewGoalsPrevious,
            (_, true) => Event::ViewChatNext,
            (_, false) => Event::ViewChatPrevious,
        };
        self.transition(event)
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
        self.checkpoint_after(|state| {
            state.transition(Event::ResolveInput {
                input: (input).to_owned(),
            })?;
            state.dispatch_resolved_input()
        })
    }

    pub fn accept_draft_input(&mut self, input: &str) -> NorthResult<()> {
        self.checkpoint_after(|state| {
            state.transition_sequence(&[
                Event::SaveDraft {
                    text: (input).to_owned(),
                },
                Event::ResolveInput {
                    input: (input).to_owned(),
                },
            ])?;
            state.dispatch_resolved_input()
        })
    }

    fn dispatch_resolved_input(&mut self) -> NorthResult<()> {
        let payload = self.input_payload.clone();
        let event = match self.input_dispatch.as_str() {
            "" => return Ok(()),
            "command-reconnect" => Event::CommandReconnect { command: payload },
            "command-usage" => Event::CommandUsage { command: payload },
            "command-compact" => Event::CommandCompact { command: payload },
            "command-new" => Event::CommandNew { command: payload },
            "command-rename" => Event::CommandRename { command: payload },
            "command-fork" => Event::CommandFork { command: payload },
            "command-archived" => Event::CommandArchived { command: payload },
            "command-archive" => Event::CommandArchive { command: payload },
            "command-resume" => Event::CommandResume { command: payload },
            "command-model" => Event::CommandModel { command: payload },
            "command-effort" => Event::CommandEffort { command: payload },
            "command-config" => Event::CommandConfig { command: payload },
            "command-quit" => Event::CommandQuit { command: payload },
            "command-delegate" => Event::CommandDelegate { command: payload },
            "command-goal" => Event::CommandGoal { command: payload },
            "command-goals" => Event::CommandGoals { command: payload },
            "command-work" => Event::CommandWork { command: payload },
            "command-agents" => Event::CommandAgents { command: payload },
            "submit-input" => Event::SubmitInput { input: payload },
            "delegate-task" => Event::DelegateTask { task: payload },
            "goal-title" => Event::GoalTitle { title: payload },
            "goal-objective" => Event::GoalObjective { objective: payload },
            "goal-edit-input" => Event::GoalEditInput { objective: payload },
            handler => {
                return Err(NorthError::State(format!(
                    "Unknown input handler {handler:?}"
                )));
            }
        };
        self.transition(event)
    }

    pub fn clear_host_effect(&mut self) -> NorthResult<()> {
        self.transition(Event::ClearHostEffect)?;
        if self.host_effect.is_empty() && self.effect_payload.is_empty() {
            Ok(())
        } else {
            Err(NorthError::Protocol(
                "Galileo retained a settled host effect".into(),
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
                "Galileo did not create and activate the requested Goal".into(),
            ));
        }
        Ok(())
    }

    pub fn select_goal(&mut self, index: usize) -> NorthResult<()> {
        let identity = self
            .goals
            .get(index)
            .map(|goal| goal.identity)
            .ok_or_else(|| NorthError::Protocol(format!("Goal index {index} is out of range")))?;
        self.transition(Event::SelectGoal { goal: identity })?;
        if self.active_goal.as_ref() != Some(&identity) {
            return Err(NorthError::Protocol(
                "Galileo did not select the requested Goal".into(),
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
            NorthError::Protocol("Galileo lost the active Goal during edit".into())
        })?;
        if active.objective() != objective
            || !active
                .prior_objectives()
                .iter()
                .any(|prior| prior == &previous)
        {
            return Err(NorthError::Protocol(
                "Galileo did not edit the active Goal with immutable history".into(),
            ));
        }
        Ok(())
    }

    pub fn observe_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        let conversation = (conversation_id).to_owned();
        self.transition(Event::ObserveConversation {
            conversation: conversation.into(),
        })
    }

    pub fn request_new_conversation(&mut self) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Ready)?;
        self.transition(Event::RequestNewConversation)?;
        self.require_conversation_change(ConversationChange::Opening)
    }

    pub fn settle_new_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Opening)?;
        let conversation = (conversation_id).to_owned();
        self.transition_sequence(&[
            Event::EnsureContext {
                conversation: conversation.clone(),
            },
            Event::SettleNewConversation {
                conversation: conversation.into(),
            },
        ])?;
        self.require_conversation_change(ConversationChange::Ready)?;
        self.require_active_conversation(conversation_id)
    }

    pub fn fail_new_conversation(&mut self) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Opening)?;
        self.transition(Event::FailNewConversation)?;
        self.require_conversation_change(ConversationChange::Ready)
    }

    pub fn request_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Ready)?;
        let conversation = (conversation_id).to_owned();
        self.transition(Event::RequestSwitchConversation {
            conversation: conversation.into(),
        })?;
        self.require_conversation_change(ConversationChange::Switching)?;
        require_identity(
            "pending conversation",
            &self.pending_conversation,
            conversation_id,
        )
    }

    pub fn settle_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Switching)?;
        let conversation = (conversation_id).to_owned();
        self.transition_sequence(&[
            Event::EnsureContext {
                conversation: conversation.clone(),
            },
            Event::SettleSwitchConversation {
                conversation: conversation.into(),
            },
        ])?;
        self.require_conversation_change(ConversationChange::Ready)?;
        self.require_active_conversation(conversation_id)
    }

    pub fn fail_switch_conversation(&mut self, conversation_id: &str) -> NorthResult<()> {
        self.require_conversation_change(ConversationChange::Switching)?;
        let conversation = (conversation_id).to_owned();
        self.transition(Event::FailSwitchConversation {
            conversation: conversation.into(),
        })?;
        self.require_conversation_change(ConversationChange::Ready)
    }

    #[cfg(test)]
    pub fn terminal_delegated_child(&self) -> Option<&str> {
        self.terminal_delegated_child.as_deref()
    }

    pub fn attach_image(&mut self) -> NorthResult<AttachmentIdentity> {
        let identity = AttachmentIdentity(self.next_attachment_number);
        self.transition(Event::AttachImage)?;
        if !self.draft_attachments.contains(&identity) {
            return Err(NorthError::Protocol(format!(
                "Galileo did not project attached image {} in the active draft",
                identity.number()
            )));
        }
        Ok(identity)
    }

    pub fn detach_image(&mut self, identity: AttachmentIdentity) -> NorthResult<()> {
        self.transition(Event::DetachImage {
            attachment: attachment_number(identity.number())?,
        })?;
        if self.draft_attachments.contains(&identity) {
            return Err(NorthError::Protocol(format!(
                "Galileo retained detached image {} in the active draft",
                identity.number()
            )));
        }
        Ok(())
    }

    pub fn submit(&mut self) -> NorthResult<Vec<AttachmentIdentity>> {
        let submitted = self.begin_turn(Event::Submit)?;
        self.require(NorthPhase::Dispatching)?;
        Ok(submitted)
    }

    pub fn settle_success(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.finish_turn(Event::SettleSuccess {
            conversation: self.active_conversation().unwrap_or_default().into(),
        })?;
        self.require(NorthPhase::Completed)
    }

    pub fn settle_failure(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Dispatching)?;
        self.finish_turn(Event::SettleFailure {
            conversation: self.active_conversation().unwrap_or_default().into(),
        })?;
        self.require(NorthPhase::Failed)
    }

    pub fn delegate(&mut self) -> NorthResult<Vec<AttachmentIdentity>> {
        let submitted = self.begin_turn(Event::Delegate)?;
        self.require(NorthPhase::Delegating)?;
        Ok(submitted)
    }

    pub fn child_spawned(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Delegating)?;
        let child = (child_id).to_owned();
        let conversation = (self.active_conversation().unwrap_or_default()).to_owned();
        self.transition(Event::ChildSpawned {
            conversation: conversation.into(),
            child: child.into(),
        })?;
        self.require(NorthPhase::Settling)?;
        self.require_active_child(child_id)
    }

    pub fn settle_delegation_success(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Settling)?;
        let child = (child_id).to_owned();
        self.finish_turn(Event::SettleDelegationSuccess {
            conversation: self.active_conversation().unwrap_or_default().into(),
            child,
        })?;
        self.require(NorthPhase::Completed)?;
        self.require_terminal_child(child_id)
    }

    pub fn fail_delegation_before_child(&mut self) -> NorthResult<()> {
        self.require(NorthPhase::Delegating)?;
        self.finish_turn(Event::FailDelegationBeforeChild {
            conversation: self.active_conversation().unwrap_or_default().into(),
        })?;
        self.require(NorthPhase::Failed)
    }

    pub fn fail_delegation_after_child(&mut self, child_id: &str) -> NorthResult<()> {
        self.require(NorthPhase::Settling)?;
        let child = (child_id).to_owned();
        self.finish_turn(Event::FailDelegationAfterChild {
            conversation: self.active_conversation().unwrap_or_default().into(),
            child,
        })?;
        self.require(NorthPhase::Failed)?;
        self.require_terminal_child(child_id)
    }

    fn begin_turn(&mut self, event: Event) -> NorthResult<Vec<AttachmentIdentity>> {
        let draft_number = self.draft_number;
        let attachments = self.draft_attachments.clone();
        let mut transitions = Vec::with_capacity(attachments.len() * 2 + 1);
        for identity in &attachments {
            transitions.push(Event::CopySubmissionAttachment {
                attachment: attachment_number(identity.number())?,
            });
            transitions.push(Event::DetachImage {
                attachment: attachment_number(identity.number())?,
            });
        }
        transitions.push(event);
        self.transition_sequence(&transitions)?;
        if self.draft_number != draft_number + 1
            || !self.draft_attachments.is_empty()
            || self.submitted_attachments != attachments
        {
            return Err(NorthError::Protocol(
                "Galileo did not atomically roll the draft into the submitted attachment set"
                    .into(),
            ));
        }
        Ok(attachments)
    }

    fn finish_turn(&mut self, event: Event) -> NorthResult<()> {
        let conversation = self.active_conversation().unwrap_or_default().to_owned();
        self.finish_turn_in(&conversation, event)
    }

    fn finish_turn_in(&mut self, conversation: &str, event: Event) -> NorthResult<()> {
        let context = self
            .conversation(conversation)
            .ok_or_else(|| NorthError::State("Turn has no conversation".into()))?;
        let mut transitions = context
            .submitted_attachments
            .iter()
            .map(|identity| {
                Ok(Event::ClearSubmittedAttachment {
                    conversation: conversation.into(),
                    attachment: attachment_number(identity.number())?,
                })
            })
            .collect::<NorthResult<Vec<_>>>()?;
        transitions.push(event);
        transitions.push(Event::ExpireSteering {
            conversation: conversation.into(),
        });
        transitions.push(Event::ClearTurn {
            conversation: conversation.into(),
        });
        self.transition_sequence(&transitions)?;
        if !self
            .conversation(conversation)
            .unwrap()
            .submitted_attachments
            .is_empty()
        {
            return Err(NorthError::Protocol(
                "Galileo retained attachments after turn settlement".into(),
            ));
        }
        Ok(())
    }

    pub fn settle_direct_in(&mut self, conversation: &str, succeeded: bool) -> NorthResult<()> {
        let context = self
            .conversation(conversation)
            .ok_or_else(|| NorthError::State("Turn has no conversation".into()))?;
        if context.phase != NorthPhase::Dispatching {
            return Err(NorthError::State(
                "Conversation has no direct turn to settle".into(),
            ));
        }
        self.finish_turn_in(
            conversation,
            if succeeded {
                Event::SettleSuccess {
                    conversation: conversation.into(),
                }
            } else {
                Event::SettleFailure {
                    conversation: conversation.into(),
                }
            },
        )
    }

    pub fn abandon_turn_in(&mut self, conversation: &str) -> NorthResult<()> {
        self.finish_turn_in(
            conversation,
            Event::AbandonTurn {
                conversation: conversation.into(),
            },
        )
    }

    pub fn settle_interrupted_in(
        &mut self,
        conversation: &str,
        child: Option<&str>,
    ) -> NorthResult<()> {
        if let Some(child) = child {
            self.transition(Event::ChildSpawned {
                conversation: conversation.into(),
                child: child.into(),
            })?;
        }
        self.finish_turn_in(
            conversation,
            Event::SettleInterrupted {
                conversation: conversation.into(),
            },
        )
    }

    pub fn settle_delegation_in(
        &mut self,
        conversation: &str,
        child: Option<&str>,
        succeeded: bool,
    ) -> NorthResult<()> {
        if let Some(child) = child {
            self.transition(Event::ChildSpawned {
                conversation: conversation.into(),
                child: child.into(),
            })?;
            self.finish_turn_in(
                conversation,
                if succeeded {
                    Event::SettleDelegationSuccess {
                        conversation: conversation.into(),
                        child: child.into(),
                    }
                } else {
                    Event::FailDelegationAfterChild {
                        conversation: conversation.into(),
                        child: child.into(),
                    }
                },
            )
        } else {
            self.finish_turn_in(
                conversation,
                Event::FailDelegationBeforeChild {
                    conversation: conversation.into(),
                },
            )
        }
    }

    fn transition(&mut self, event: Event) -> NorthResult<()> {
        self.transition_sequence(&[event])
    }

    fn transition_sequence(&mut self, events: &[Event]) -> NorthResult<()> {
        if self.storage_failed {
            return Err(NorthError::Configuration("Saving workspace data failed; close North and resolve the storage error before continuing".into()));
        }
        self.workbench.run(events)?;
        self.accept_state()
    }

    fn accept_state(&mut self) -> NorthResult<()> {
        let state = &self.workbench;
        self.revision = Some(state.revision);
        self.menu = menu_view(&state.menu);
        self.usage = UsagePanel {
            rows: state
                .usage_rows
                .iter()
                .filter(|row| row.visible)
                .map(|row| row.row.clone())
                .collect(),
            empty: state.usage_empty.clone(),
            unknown: state.usage_unknown.clone(),
            help: state.usage_help.clone(),
        };
        self.references = state
            .references
            .iter()
            .map(|reference| crate::references::Reference {
                name: reference.name.clone(),
                description: reference.description.clone(),
                kind: reference.kind.clone(),
                path: reference.path.clone().into(),
            })
            .collect();
        self.reference_selection = state.reference_selection as usize;
        self.chat = state.chat.clone();
        self.connection_state = state.connection_state.clone();
        self.reference_query = state.reference_query.clone();
        self.references_open = state.references_open;
        self.contexts = state.contexts.clone();
        self.pending_inputs = state.pending_inputs.clone();
        self.prompts = state.prompts.clone();
        self.next_prompt_number = state.next_prompt_number;
        self.effect_input_number = state.effect_input_number;
        self.conversation_change = state.conversation_change;
        self.draft_number = state.draft_number;
        self.next_attachment_number = state.next_attachment_number;
        self.goals = state.goals.clone();
        self.active_goal = state.active_goal;
        self.commands = state.commands.clone();
        self.views = state.views.clone();
        self.input_handler = state.input_handler.clone();
        self.input_dispatch = state.input_dispatch.clone();
        self.input_payload = state.input_payload.clone();
        self.input_kind = state.input_kind.clone();
        self.host_effect = state.host_effect.clone();
        self.effect_payload = state.effect_payload.clone();
        self.notice = state.notice.clone();
        self.active_view = state.active_view.clone();
        self.next_view_handler = state.next_view_handler.clone();
        self.previous_view_handler = state.previous_view_handler.clone();
        self.active_conversation =
            (!state.active_conversation.is_empty()).then(|| state.active_conversation.clone());
        self.pending_conversation =
            (!state.pending_conversation.is_empty()).then(|| state.pending_conversation.clone());
        let active = state
            .contexts
            .iter()
            .find(|context| context.id == state.active_conversation)
            .ok_or_else(|| {
                NorthError::State(format!(
                    "Selected conversation {:?} is missing",
                    state.active_conversation
                ))
            })?;
        self.active_turn = active.active_turn.clone();
        self.phase = active.phase;
        self.active_delegated_child = active.active_child.clone();
        self.terminal_delegated_child = active.terminal_child.clone();
        self.draft_attachments = active.draft_attachments.clone();
        self.submitted_attachments = active.submitted_attachments.clone();
        if self.checkpoint_depth == 0 {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Keep intermediate admissions private to a synchronous host operation.
    /// Its last admitted state is durable before any result reaches the caller,
    /// including when a later step rejects the operation.
    pub(crate) fn checkpoint_after<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> NorthResult<T>,
    ) -> NorthResult<T> {
        let before = self.revision;
        self.checkpoint_depth += 1;
        let result = operation(self);
        self.checkpoint_depth -= 1;
        if self.checkpoint_depth == 0 && self.revision != before {
            self.checkpoint()?;
        }
        result
    }

    fn checkpoint(&mut self) -> NorthResult<()> {
        if let Some(store) = &mut self.store {
            let result = self
                .workbench
                .snapshot()
                .and_then(|segments| store.checkpoint(&segments));
            if result.is_err() {
                self.storage_failed = true;
            }
            result?;
        }
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

fn require_identity(label: &str, observed: &Option<String>, expected: &str) -> NorthResult<()> {
    if observed.as_deref() == Some(expected) {
        return Ok(());
    }
    Err(NorthError::Protocol(format!(
        "conversation state projected {label} {:?}, expected {expected}",
        observed.as_deref()
    )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_commands_require_archive_confirmation_and_pause_archived_work() {
        let mut state = NorthState::open().unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("alpha").unwrap();
        state.accept_input("/rename Ångström notes").unwrap();
        assert_eq!(state.host_effect().unwrap().action(), "rename-conversation");
        assert_eq!(state.host_effect().unwrap().payload(), "Ångström notes");
        state.clear_host_effect().unwrap();
        state.accept_input("/fork").unwrap();
        assert_eq!(state.host_effect().unwrap().payload(), "alpha");
        state.clear_host_effect().unwrap();
        state.accept_input("/archive").unwrap();
        assert!(state.host_effect().is_none());
        state.submit().unwrap();
        state.accept_input("/archive confirm").unwrap();
        assert!(state.host_effect().is_none());
        state.settle_success().unwrap();
        let queued = state.queue_input("keep this follow-up").unwrap();
        state.accept_input("/archive confirm").unwrap();
        assert_eq!(state.host_effect().unwrap().action(), "archive-conversation");
        state.clear_host_effect().unwrap();
        state.observe_archived("alpha").unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), None);
        state.observe_restored("alpha").unwrap();
        assert_eq!(state.prepare_queued_input().unwrap(), Some(queued));
    }

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
        state.query_references("file-255", &candidates).unwrap();
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
    fn chat_batch_preserves_delta_order_and_final_replacement() {
        let mut state = NorthState::open().unwrap();
        state.request_new_conversation().unwrap();
        state.settle_new_conversation("batch").unwrap();
        let entry = |text, append| ChatEntryInput { conversation: "batch", turn: "turn", key: "item", kind: "agentMessage", text, status: "completed", append };
        state.observe_chat_items(&[entry("first", false), entry(" delta", true)]).unwrap();
        assert_eq!(state.chat().len(), 1);
        assert_eq!(state.chat()[0].text, "first delta");
        state.observe_chat_items(&[entry("ignored delta", true), entry("final", false)]).unwrap();
        assert_eq!(state.chat().len(), 1);
        assert_eq!(state.chat()[0].text, "final");
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
    fn state_admits_repeated_turns_after_success_and_failure() {
        let mut state = NorthState::open().expect("North Galileo program opens");
        assert_eq!(state.phase(), NorthPhase::Idle);
        state.submit().expect("first submit is admitted");
        assert_eq!(state.phase(), NorthPhase::Dispatching);
        state.settle_success().expect("first turn settles");
        assert_eq!(state.phase(), NorthPhase::Completed);
        state.submit().expect("second submit is admitted");
        state.settle_failure().expect("second turn settles");
        assert_eq!(state.phase(), NorthPhase::Failed);
        state
            .submit()
            .expect("third submit is admitted after failure");
        assert_eq!(state.phase(), NorthPhase::Dispatching);
    }



    #[test]
    fn state_creates_orders_selects_and_edits_goals_with_revisions() {
        let mut state = NorthState::open().expect("North Galileo program opens");
        assert!(state.goals().is_empty());
        assert!(state.active_goal().is_none());

        state
            .create_goal("Build North", "Make Galileo own Goals")
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
            .edit_active_goal("Make Galileo own ordered Goal revisions")
            .expect("first edit is admitted");
        state
            .edit_active_goal("Make Galileo own all Goal semantics")
            .expect("repeated edit is admitted");
        let active = state.active_goal().expect("selected Goal remains active");
        assert_eq!(active.title(), "Build North");
        assert_eq!(active.objective(), "Make Galileo own all Goal semantics");
        assert_eq!(
            active.prior_objectives(),
            [
                "Make Galileo own Goals",
                "Make Galileo own ordered Goal revisions"
            ]
        );
    }

    #[test]
    fn state_owns_image_identity_draft_rollover_and_submission_membership() {
        let mut state = NorthState::open().expect("North Galileo program opens");
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
    fn state_owns_delegation_admission_and_settlement() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        let mut state = NorthState::open().expect("North Galileo program opens");
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
    fn state_owns_delegation_failure_before_and_after_spawn() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        let mut before = NorthState::open().expect("North Galileo program opens");
        before.delegate().expect("delegation is admitted");
        before
            .fail_delegation_before_child()
            .expect("pre-child failure is admitted");
        assert_eq!(before.phase(), NorthPhase::Failed);

        let mut after = NorthState::open().expect("North Galileo program opens");
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
    fn state_rejects_settlement_for_a_different_child() {
        const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
        const OTHER: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a3";
        let mut state = NorthState::open().expect("North Galileo program opens");
        state.delegate().expect("delegation is admitted");
        state
            .child_spawned(CHILD)
            .expect("child receipt is admitted");

        let error = state
            .settle_delegation_success(OTHER)
            .expect_err("another child cannot settle this delegation");
        assert!(matches!(error, NorthError::Protocol(_)));
        assert_eq!(state.phase(), NorthPhase::Settling);
        assert_eq!(state.active_delegated_child(), Some(CHILD));
        assert_eq!(state.terminal_delegated_child(), None);
    }




}

impl NorthState {
    pub fn open_history(
        &mut self,
        conversations: &[crate::codex::ConversationOption],
        query: &str,
        archived: bool,
    ) -> NorthResult<()> {
        let mut events = vec![
            Event::ClearMenuRows,
            Event::OpenHistoryMenu {
                kind: if archived {
                    "archived-history"
                } else {
                    "history"
                }
                .into(),
            },
        ];
        events.extend(conversations.iter().map(|conversation| Event::OfferRow {
            key: conversation.id.clone(),
            label: conversation.title.clone(),
            description: conversation.preview.clone(),
            annotation: String::new(),
        }));
        events.extend([
            Event::QueryMenu {
                query: query.into(),
            },
            Event::FilterMenu,
            Event::CountMenu,
        ]);
        self.transition_sequence(&events)
    }

    pub fn open_settings_menu(
        &mut self,
        kind: &str,
        title: &str,
        rows: &[(&str, &str, &str, &str)],
    ) -> NorthResult<()> {
        self.settings_menu(kind, title, rows, false)
    }
    pub fn finish_settings_effect(
        &mut self,
        kind: &str,
        title: &str,
        rows: &[(&str, &str, &str, &str)],
    ) -> NorthResult<()> {
        self.settings_menu(kind, title, rows, true)
    }

    fn settings_menu(
        &mut self,
        kind: &str,
        title: &str,
        rows: &[(&str, &str, &str, &str)],
        finish_effect: bool,
    ) -> NorthResult<()> {
        let mut events = vec![
            Event::ClearMenuRows,
            Event::OpenSettingsMenu {
                kind: kind.into(),
                title: title.into(),
            },
        ];
        events.extend(
            rows.iter()
                .map(|(key, label, description, annotation)| Event::OfferRow {
                    key: (*key).into(),
                    label: (*label).into(),
                    description: (*description).into(),
                    annotation: (*annotation).into(),
                }),
        );
        events.extend([
            Event::QueryMenu {
                query: String::new(),
            },
            Event::CountMenu,
        ]);
        if finish_effect {
            events.push(Event::ClearHostEffect);
        }
        self.transition_sequence(&events)
    }

    pub fn work_menu(&mut self, rows: &[crate::work_items::WorkRow]) -> NorthResult<()> {
        let mut events = vec![
            Event::ClearMenuRows,
            Event::OpenSettingsMenu {
                kind: "work".into(),
                title: "Work items".into(),
            },
        ];
        events.extend(rows.iter().map(|row| {
            let (label, description, annotation) = row.menu_texts();
            Event::OfferRow {
                key: row.reference.clone(),
                label,
                description,
                annotation: annotation.into(),
            }
        }));
        events.extend([
            Event::QueryMenu {
                query: String::new(),
            },
            Event::CountMenu,
        ]);
        self.transition_sequence(&events)
    }

    pub fn query_menu(&mut self, query: &str) -> NorthResult<()> {
        self.transition_sequence(&[
            Event::QueryMenu {
                query: query.into(),
            },
            Event::FilterMenu,
            Event::CountMenu,
        ])
    }

    pub fn move_menu(&mut self, delta: isize) -> NorthResult<()> {
        self.transition_sequence(&[Event::MoveMenu {
            delta: delta as i64,
        }])
    }
    pub fn close_menu(&mut self) -> NorthResult<()> {
        self.transition_sequence(&[Event::CloseMenu])
    }
    pub fn accept_menu(&mut self) -> NorthResult<()> {
        self.transition_sequence(&[Event::AcceptMenu])
    }
}

fn numeric_argument(number: u64) -> NorthResult<i64> {
    if number > MAX_EXACT_F64_INTEGER {
        return Err(NorthError::State(
            "Numeric observation exceeds exact range".into(),
        ));
    }
    Ok(number as i64)
}
fn attachment_number(number: u64) -> NorthResult<i64> {
    if number > MAX_EXACT_F64_INTEGER {
        return Err(NorthError::Protocol(
            "Galileo attachment identity exhausted the exact F64 integer domain".into(),
        ));
    }
    Ok(number as i64)
}
fn exact_integer(number: f64) -> NorthResult<i64> {
    if number.is_finite() && number.fract() == 0.0 && number.abs() <= MAX_EXACT_F64_INTEGER as f64 {
        Ok(number as i64)
    } else {
        Err(NorthError::State(
            "Observation is not an exact integer".into(),
        ))
    }
}
fn menu_view(menu: &crate::state_core::Menu) -> MenuState {
    MenuState {
        kind: menu.kind.clone(),
        title: menu.title.clone(),
        query: menu.query.clone(),
        help: menu.help.clone(),
        empty: menu.empty.clone(),
        selection: menu.selection,
        rows: menu
            .rows
            .iter()
            .filter(|row| row.visible)
            .map(|row| crate::menu::MenuRow {
                key: row.key.clone(),
                label: row.label.clone(),
                description: row.description.clone(),
                annotation: row.annotation.clone(),
                position: row.position,
            })
            .collect::<Vec<_>>()
            .into(),
    }
}
