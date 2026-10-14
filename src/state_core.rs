use crate::error::{NorthError, NorthResult};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NorthPhase {
    Idle,
    Dispatching,
    Delegating,
    Settling,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConversationChange {
    Ready,
    Opening,
    Switching,
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct AttachmentIdentity(pub u64);

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Goal {
    pub identity: u64,
    pub title: String,
    pub objective: String,
    pub status: String,
    pub order: u64,
    pub prior_objectives: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandSpec {
    pub name: String,
    pub description: String,
    pub order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewSpec {
    pub name: String,
    pub label: String,
    pub order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChatEntry {
    pub visible: bool,
    pub conversation: String,
    pub turn: String,
    pub key: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub style: String,
    pub order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PendingInput {
    pub number: u64,
    pub conversation: String,
    pub text: String,
    pub status: String,
    pub attachments: Vec<AttachmentIdentity>,
    pub client_id: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub readiness: TurnReadiness,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub turn: String,
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptOption {
    pub value: String,
    pub checked: bool,
    pub number: u64,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsagePanel {
    pub rows: Vec<UsageRow>,
    pub empty: String,
    pub unknown: String,
    pub help: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRow {
    pub label: String,
    pub value: f64,
    pub known: bool,
    pub unit: String,
    pub order: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TurnReadiness {
    Ready,
    Active,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    ConnectionLost,
    PauseDisconnectedInputs,
    DisconnectPrompts,
    BeginReconnect,
    FinishReconnect {
        success: bool,
    },
    CommandReconnect {
        command: String,
    },
    BeginConversationReconciliation {
        conversation: String,
    },
    CountUsage,
    PresentUsage,
    WithdrawUsageRow {
        owner: String,
        key: String,
    },
    ObserveContextUsage {
        conversation: String,
        used: i64,
        window: i64,
        total: i64,
    },
    ObserveQuotaWindow {
        key: String,
        name: String,
        used: i64,
        duration: i64,
        reset: i64,
        now: i64,
    },
    ObserveResetCredits {
        count: i64,
        known: bool,
    },
    CommandUsage {
        command: String,
    },
    CompactionCompleted,
    CommandCompact {
        command: String,
    },
    ClearMenuRows,
    OpenHistoryMenu {
        kind: String,
    },
    OpenSettingsMenu {
        kind: String,
        title: String,
    },
    AcceptMenu,
    QueryMenu {
        query: String,
    },
    FilterMenu,
    CountMenu,
    MoveMenu {
        delta: i64,
    },
    CloseMenu,
    ClearReferences,
    QueryReferences {
        query: String,
    },
    OfferReference {
        name: String,
        description: String,
        kind: String,
        path: String,
    },
    MoveReference {
        delta: i64,
    },
    DismissReferences,
    FilterTranscript,
    SearchTranscript {
        query: String,
    },
    ScrollTranscript {
        delta: i64,
    },
    SizeTranscript {
        limit: i64,
    },
    ToggleChanges,
    ObserveSettings {
        conversation: String,
        model: String,
        effort: String,
    },
    EnsureContext {
        conversation: String,
    },
    SaveDraft {
        text: String,
    },
    ObservePrompt {
        request: String,
        conversation: String,
        turn: String,
        kind: String,
        cancellation: String,
    },
    ObservePromptQuestion {
        number: i64,
        position: i64,
        key: String,
        header: String,
        text: String,
        secret: bool,
        other: bool,
        codec: String,
        multiple: bool,
        optional: bool,
    },
    ObservePromptOption {
        number: i64,
        position: i64,
        index: i64,
        label: String,
        description: String,
        value: String,
    },
    FocusPrompts,
    PresentPromptQuestions,
    NavigatePrompt {
        direction: i64,
    },
    AnswerPromptChoice,
    TogglePromptOption,
    AnswerPromptMultiple {
        text: String,
    },
    SkipPromptQuestion,
    PromptAnswerProblem {
        problem: String,
    },
    AnswerPromptText {
        text: String,
    },
    PreparePromptResponse,
    CancelPrompt,
    PromptResponseWritten {
        number: i64,
        result: String,
    },
    ResolvePrompt {
        conversation: String,
        request: String,
    },
    CloseTurnPrompts {
        conversation: String,
        turn: String,
    },
    ClearResolvedAnswers,
    ObserveTurn {
        conversation: String,
        turn: String,
    },
    ObserveStoredTurn {
        conversation: String,
        turn: String,
        status: String,
    },
    ClearTurn {
        conversation: String,
    },
    FinishObservedTurn {
        conversation: String,
        turn: String,
    },
    ExpireSteering {
        conversation: String,
    },
    RetainInput {
        text: String,
        mode: String,
    },
    BindInputClientId {
        number: i64,
        client: String,
    },
    ObserveInputAcceptance {
        conversation: String,
        client: String,
    },
    RecordInputAcceptance {
        number: i64,
        result: String,
    },
    InputReceipt {
        number: i64,
        result: String,
    },
    PrepareSteering,
    RetainQueuedAttachment {
        number: i64,
        attachment: i64,
    },
    PrepareQueuedInput,
    SubmitQueued {
        number: i64,
    },
    CopyQueuedToSubmitted {
        number: i64,
        attachment: i64,
    },
    PrepareInputEdit,
    RestoreInputAttachment {
        number: i64,
        attachment: i64,
    },
    ForgetInputAttachment {
        number: i64,
        attachment: i64,
    },
    ForgetInput {
        number: i64,
    },
    AppendChat {
        conversation: String,
        kind: String,
        text: String,
    },
    ObserveChatItem {
        conversation: String,
        turn: String,
        key: String,
        kind: String,
        text: String,
        status: String,
        mode: String,
    },
    ClearChat,
    Initialize,
    CommandWork {
        command: String,
    },
    Inspect,
    ResolveInput {
        input: String,
    },
    SelectGoal {
        goal: u64,
    },
    CommandNew {
        command: String,
    },
    CommandResume {
        command: String,
    },
    CommandRename {
        command: String,
    },
    CommandFork {
        command: String,
    },
    CommandArchived {
        command: String,
    },
    CommandArchive {
        command: String,
    },
    ObserveArchived {
        conversation: String,
    },
    ObserveRestored {
        conversation: String,
    },
    HistoryActionCompleted {
        action: String,
    },
    CommandModel {
        command: String,
    },
    CommandEffort {
        command: String,
    },
    CommandConfig {
        command: String,
    },
    CommandQuit {
        command: String,
    },
    CommandDelegate {
        command: String,
    },
    DelegateTask {
        task: String,
    },
    SubmitInput {
        input: String,
    },
    ClearHostEffect,
    CommandGoal {
        command: String,
    },
    GoalTitle {
        title: String,
    },
    GoalObjective {
        objective: String,
    },
    GoalEditInput {
        objective: String,
    },
    CommandGoals {
        command: String,
    },
    CommandAgents {
        command: String,
    },
    ShowChat,
    ViewChatNext,
    ViewChatPrevious,
    ViewGoalsNext,
    ViewGoalsPrevious,
    ObserveConversation {
        conversation: String,
    },
    AttachImage,
    DetachImage {
        attachment: i64,
    },
    CopySubmissionAttachment {
        attachment: i64,
    },
    ClearSubmittedAttachment {
        conversation: String,
        attachment: i64,
    },
    RequestNewConversation,
    SettleNewConversation {
        conversation: String,
    },
    FailNewConversation,
    RequestSwitchConversation {
        conversation: String,
    },
    SettleSwitchConversation {
        conversation: String,
    },
    FailSwitchConversation {
        conversation: String,
    },
    Submit,
    Delegate,
    ChildSpawned {
        conversation: String,
        child: String,
    },
    SettleSuccess {
        conversation: String,
    },
    AbandonTurn {
        conversation: String,
    },
    SettleInterrupted {
        conversation: String,
    },
    SettleFailure {
        conversation: String,
    },
    SettleDelegationSuccess {
        conversation: String,
        child: String,
    },
    FailDelegationBeforeChild {
        conversation: String,
    },
    FailDelegationAfterChild {
        conversation: String,
        child: String,
    },
    OfferRow {
        key: String,
        label: String,
        description: String,
        annotation: String,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuRow {
    pub key: String,
    pub label: String,
    pub description: String,
    pub annotation: String,
    pub order: i64,
    pub position: i64,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    pub kind: String,
    pub title: String,
    pub query: String,
    pub help: String,
    pub empty: String,
    pub selection: i64,
    pub count: i64,
    pub row_count: i64,
    pub rows: Vec<MenuRow>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub number: i64,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredUsageRow {
    pub key: String,
    pub owner: String,
    pub row: UsageRow,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub revision: u64,
    pub connection_state: String,
    pub menu: Menu,
    pub usage_rows: Vec<StoredUsageRow>,
    pub usage_next_row: u64,
    pub usage_empty: String,
    pub usage_unknown: String,
    pub usage_help: String,
    pub references: Vec<Reference>,
    pub reference_count: i64,
    pub reference_query: String,
    pub reference_selection: i64,
    pub references_open: bool,
    pub contexts: Vec<ConversationState>,
    pub known_conversations: Vec<String>,
    #[serde(with = "chat_snapshot")]
    pub chat: std::sync::Arc<Vec<ChatEntry>>,
    pub next_chat_order: u64,
    pub pending_inputs: Vec<PendingInput>,
    pub prompts: Vec<Prompt>,
    pub next_prompt_number: u64,
    pub effect_input_number: u64,
    pub conversation_change: ConversationChange,
    pub active_conversation: String,
    pub pending_conversation: String,
    pub draft_number: u64,
    pub next_attachment_number: u64,
    pub goals: Vec<Goal>,
    pub active_goal: Option<u64>,
    pub next_goal_order: u64,
    pub commands: Vec<CommandSpec>,
    pub views: Vec<ViewSpec>,
    pub initialized: bool,
    pub input_handler: String,
    pub input_dispatch: String,
    pub input_payload: String,
    pub input_kind: String,
    pub pending_goal_title: String,
    pub host_effect: String,
    pub effect_payload: String,
    pub notice: String,
    pub active_view: String,
    pub next_view_handler: String,
    pub previous_view_handler: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            revision: 0,
            connection_state: "connected".into(),
            menu: Menu {
                kind: String::new(),
                title: String::new(),
                query: String::new(),
                help: "Type to search · ↑/↓ choose · Enter open · Esc close".into(),
                empty: "No matching conversations".into(),
                selection: 0,
                count: 0,
                row_count: 0,
                rows: Vec::new(),
            },
            usage_rows: Vec::new(),
            usage_next_row: 0,
            usage_empty: "Usage has not been reported yet.".into(),
            usage_unknown: "not reported".into(),
            usage_help: "↑/↓ scroll · Esc close · /compact makes room · /usage refreshes".into(),
            references: Vec::new(),
            reference_count: 0,
            reference_query: String::new(),
            reference_selection: 0,
            references_open: false,
            contexts: vec![ConversationState::new(String::new())],
            known_conversations: Vec::new(),
            chat: Default::default(),
            next_chat_order: 1,
            pending_inputs: Vec::new(),
            prompts: Vec::new(),
            next_prompt_number: 1,
            effect_input_number: 1,
            conversation_change: ConversationChange::Ready,
            active_conversation: String::new(),
            pending_conversation: String::new(),
            draft_number: 1,
            next_attachment_number: 1,
            goals: Vec::new(),
            active_goal: None,
            next_goal_order: 1,
            commands: Vec::new(),
            views: vec![
                ViewSpec {
                    name: "chat".into(),
                    label: "Chat".into(),
                    order: 1,
                },
                ViewSpec {
                    name: "goals".into(),
                    label: "Goals".into(),
                    order: 2,
                },
            ],
            initialized: false,
            input_handler: "submit-input".into(),
            input_dispatch: String::new(),
            input_payload: String::new(),
            input_kind: "message".into(),
            pending_goal_title: String::new(),
            host_effect: String::new(),
            effect_payload: String::new(),
            notice: String::new(),
            active_view: "chat".into(),
            next_view_handler: "view-chat-next".into(),
            previous_view_handler: "view-chat-previous".into(),
        }
    }
}

impl ConversationState {
    fn new(id: String) -> Self {
        Self {
            id,
            transcript_query: String::new(),
            transcript_offset: 0,
            transcript_limit: 0,
            transcript_changes: false,
            model: "Codex default".into(),
            effort: "default".into(),
            attached: false,
            phase: NorthPhase::Idle,
            active_turn: String::new(),
            active_child: None,
            terminal_child: None,
            draft_attachments: Vec::new(),
            submitted_attachments: Vec::new(),
            saved_draft: String::new(),
            readiness: TurnReadiness::Ready,
        }
    }
}

const SNAPSHOT_MAGIC: &[u8] = b"NORTH-RUST-STATE\0\x01";

impl State {
    pub fn open(bytes: Option<&[u8]>) -> NorthResult<Self> {
        let Some(bytes) = bytes else {
            return Ok(Self::default());
        };
        let invalid = || {
            NorthError::State(
                "Saved state is corrupt or has an unsupported schema; the file has been preserved"
                    .into(),
            )
        };
        let body_len = bytes.len().checked_sub(32).ok_or_else(invalid)?;
        let (body, checksum) = bytes.split_at(body_len);
        if !body.starts_with(SNAPSHOT_MAGIC) || Sha256::digest(body).as_slice() != checksum {
            return Err(invalid());
        }
        serde_json::from_slice(&body[SNAPSHOT_MAGIC.len()..]).map_err(|_| invalid())
    }

    pub fn snapshot(&self) -> NorthResult<Vec<std::sync::Arc<[u8]>>> {
        let mut bytes = SNAPSHOT_MAGIC.to_vec();
        bytes.extend(serde_json::to_vec(self)?);
        let checksum = Sha256::digest(&bytes);
        bytes.extend(checksum);
        Ok(vec![bytes.into()])
    }

    pub fn run(&mut self, events: &[Event]) -> NorthResult<()> {
        let mut candidate = self.clone();
        for event in events {
            candidate.step(event)?;
        }
        for event in [
            Event::ClearResolvedAnswers,
            Event::FocusPrompts,
            Event::PresentPromptQuestions,
            Event::FilterTranscript,
            Event::PresentUsage,
            Event::CountUsage,
        ] {
            candidate.step(&event)?;
        }
        candidate.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| NorthError::State("State revision exhausted".into()))?;
        *self = candidate;
        Ok(())
    }

    fn context_mut(&mut self, id: &str) -> Option<&mut ConversationState> {
        self.contexts.iter_mut().find(|context| context.id == id)
    }
    fn selected_context_mut(&mut self) -> Option<&mut ConversationState> {
        let id = self.active_conversation.clone();
        self.context_mut(&id)
    }
    fn selected_goal_mut(&mut self) -> Option<&mut Goal> {
        self.goals
            .iter_mut()
            .find(|goal| Some(goal.identity) == self.active_goal)
    }
    fn effect(&mut self, action: &str, payload: String) {
        self.host_effect = action.into();
        self.effect_payload = payload;
    }
    fn chat_entry(
        &mut self,
        conversation: String,
        turn: String,
        key: String,
        kind: String,
        text: String,
        status: String,
    ) {
        std::sync::Arc::make_mut(&mut self.chat).push(ChatEntry {
            visible: false,
            conversation,
            turn,
            key,
            style: chat_appearance(&kind, &status).into(),
            kind,
            text,
            status,
            order: self.next_chat_order,
        });
        self.next_chat_order += 1;
    }

    fn step(&mut self, event: &Event) -> NorthResult<()> {
        use Event::*;
        match event {
            ConnectionLost => {
                self.connection_state = "disconnected".into();
                self.notice = "Connection lost. Drafts and queued messages are kept; unconfirmed messages will not be resent.".into();
            }
            PauseDisconnectedInputs => {
                for input in &mut self.pending_inputs {
                    input.status = match input.status.as_str() {
                        "queued" => "paused",
                        "waiting to steer" => "not sent",
                        "sending" | "submitted" | "sending correction" => "delivery unknown",
                        other => other,
                    }
                    .into();
                }
            }
            DisconnectPrompts => {
                for prompt in &mut self.prompts {
                    if prompt.live {
                        prompt.live = false;
                        prompt.status = match prompt.status.as_str() {
                            "resolved" => "resolved",
                            "waiting" => "disconnected",
                            _ => "delivery unknown",
                        }
                        .into();
                    }
                }
            }
            BeginReconnect => {
                if self.connection_state == "disconnected" {
                    self.connection_state = "connecting".into();
                    self.notice = "Reconnecting…".into();
                }
            }
            FinishReconnect { success } => {
                self.connection_state = if *success { "connected" } else { "failed" }.into();
                self.notice = if *success {
                    "Reconnected. Paused messages need review; Alt+↑ edits the next unsent message."
                } else {
                    "Could not reconnect. Drafts are kept. Use /reconnect to try again."
                }
                .into();
            }
            CommandReconnect { .. } => {
                self.notice = if self.connection_state == "connected" {
                    "Already connected."
                } else {
                    "Reconnecting…"
                }
                .into();
                if self.connection_state == "failed" {
                    self.connection_state = "disconnected".into();
                }
            }
            BeginConversationReconciliation { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    context.phase = NorthPhase::Idle;
                    context.readiness = TurnReadiness::Ready;
                    context.active_turn.clear();
                }
            }
            EnsureContext { conversation } => {
                if !self
                    .contexts
                    .iter()
                    .any(|context| context.id == *conversation)
                {
                    self.contexts
                        .push(ConversationState::new(conversation.clone()));
                }
            }
            ObserveSettings {
                conversation,
                model,
                effort,
            } => {
                if let Some(context) = self.context_mut(conversation) {
                    context.model = model.clone();
                    context.effort = effort.clone();
                    context.attached = true;
                }
            }
            SaveDraft { text } => {
                if let Some(context) = self.selected_context_mut() {
                    context.saved_draft = text.clone();
                }
            }
            SearchTranscript { query } => {
                if let Some(context) = self.selected_context_mut() {
                    context.transcript_query = query.clone();
                    context.transcript_offset = 0;
                }
            }
            ScrollTranscript { delta } => {
                if let Some(context) = self.selected_context_mut() {
                    context.transcript_offset = (context.transcript_offset as i64 + delta)
                        .clamp(0, context.transcript_limit as i64)
                        as u64;
                }
            }
            SizeTranscript { limit } => {
                if let Some(context) = self.selected_context_mut() {
                    context.transcript_limit = *limit as u64;
                    context.transcript_offset = context.transcript_offset.min(*limit as u64);
                }
            }
            ToggleChanges => {
                if let Some(context) = self.selected_context_mut() {
                    context.transcript_changes = !context.transcript_changes;
                    context.transcript_offset = 0;
                }
            }
            FilterTranscript => {
                if let Some(context) = self
                    .contexts
                    .iter()
                    .find(|context| context.id == self.active_conversation)
                {
                    let query = context.transcript_query.to_lowercase();
                    let visible = |entry: &ChatEntry| {
                        entry.conversation == self.active_conversation
                            && (!context.transcript_changes
                                || matches!(entry.kind.as_str(), "fileChange" | "diff"))
                            && entry.text.to_lowercase().contains(&query)
                    };
                    if self
                        .chat
                        .iter()
                        .any(|entry| entry.visible != visible(entry))
                    {
                        for entry in std::sync::Arc::make_mut(&mut self.chat) {
                            entry.visible = visible(entry);
                        }
                    }
                }
            }
            CountUsage => {
                if self.menu.kind == "usage" {
                    self.menu.count =
                        self.usage_rows.iter().filter(|row| row.visible).count() as i64;
                    if self.menu.selection >= self.menu.count {
                        self.menu.selection = (self.menu.count - 1).max(0);
                    }
                }
            }
            PresentUsage => {
                for row in &mut self.usage_rows {
                    row.visible = row.owner.is_empty() || row.owner == self.active_conversation;
                }
            }
            WithdrawUsageRow { owner, key } => self
                .usage_rows
                .retain(|row| row.owner != *owner || row.key != *key),
            ObserveContextUsage {
                conversation,
                used,
                window,
                total,
            } => {
                self.add_usage(
                    "context",
                    conversation,
                    "Context used",
                    if *window > 0 { used * 100 / window } else { 0 },
                    *window > 0,
                    "%",
                );
                self.add_usage(
                    "tokens",
                    conversation,
                    "Conversation total",
                    *total,
                    true,
                    " tokens",
                );
            }
            ObserveQuotaWindow {
                key,
                name,
                used,
                duration,
                reset,
                now,
            } => {
                self.add_usage(
                    key,
                    "",
                    &format!("{name} — remaining"),
                    if *used > 100 { 0 } else { 100 - used },
                    true,
                    "%",
                );
                self.add_usage(
                    key,
                    "",
                    &format!("{name} — window"),
                    *duration,
                    *duration > 0,
                    " min",
                );
                self.add_usage(
                    key,
                    "",
                    &format!("{name} — next reset"),
                    if reset > now { (reset - now) / 60 } else { 0 },
                    *reset > 0,
                    " min from update",
                );
            }
            ObserveResetCredits { count, known } => self.add_usage(
                "reset-credits",
                "",
                "Available rate-limit resets",
                *count,
                *known,
                "",
            ),
            CommandUsage { .. } => {
                if self.host_effect.is_empty() {
                    self.menu.kind = "usage".into();
                    self.menu.title = "Context and usage".into();
                    self.effect("read-usage", String::new());
                }
            }
            CompactionCompleted => self.notice = "Conversation now has more room.".into(),
            CommandCompact { command } => {
                if command == "/compact" {
                    let empty_effect = self.host_effect.is_empty();
                    if let Some(context) = self.selected_context_mut() {
                        if context.readiness != TurnReadiness::Ready {
                            self.notice =
                                "Finish or interrupt the current request before making more room."
                                    .into();
                        } else if !context.id.is_empty() && empty_effect {
                            context.readiness = TurnReadiness::Active;
                            context.phase = NorthPhase::Dispatching;
                            let id = context.id.clone();
                            self.effect("compact-conversation", id);
                            self.menu.kind.clear();
                            self.notice = "Making room in this conversation…".into();
                        }
                    }
                }
            }
            ClearMenuRows => self.menu.rows.clear(),
            OpenHistoryMenu { kind } => {
                self.menu.kind = kind.clone();
                self.menu.title = if kind == "archived-history" {
                    "Archived Conversations"
                } else {
                    "Resume Conversation"
                }
                .into();
                self.menu.row_count = 0;
            }
            OpenSettingsMenu { kind, title } => {
                self.menu.kind = kind.clone();
                self.menu.title = title.clone();
                self.menu.row_count = 0;
                self.menu.help = match kind.as_str() { "document" => "↑/↓ scroll · Esc close · /work claim, release, need, unneed or close to edit", "work" => "Type to filter · ↑/↓ choose · Enter open · Esc close · /work search TEXT includes closed issues", _ => "Type to search · ↑/↓ choose · Enter open · Esc close" }.into();
                self.menu.empty = if kind == "work" {
                    "No matching work items"
                } else {
                    "No matching entries"
                }
                .into();
            }
            OfferRow {
                key,
                label,
                description,
                annotation,
            } => {
                self.menu.rows.push(MenuRow {
                    key: key.clone(),
                    label: label.clone(),
                    description: description.clone(),
                    annotation: if matches!(self.menu.kind.as_str(), "history" | "archived-history")
                        && *key == self.active_conversation
                    {
                        "current".into()
                    } else {
                        annotation.clone()
                    },
                    order: self.menu.row_count,
                    position: self.menu.row_count,
                    visible: true,
                });
                self.menu.row_count += 1;
            }
            QueryMenu { query } => {
                self.menu.query = query.clone();
                self.menu.selection = 0;
            }
            FilterMenu => {
                let query = self.menu.query.to_lowercase();
                let rows = self.menu.rows.clone();
                for row in &mut self.menu.rows {
                    row.visible = menu_matches(row, &query);
                    row.position = rows
                        .iter()
                        .filter(|prior| prior.order < row.order && menu_matches(prior, &query))
                        .count() as i64;
                }
            }
            CountMenu => {
                self.menu.count = self.menu.rows.iter().filter(|row| row.visible).count() as i64
            }
            MoveMenu { delta } => {
                if self.menu.count > 0 {
                    self.menu.selection = wrapped(self.menu.selection, *delta, self.menu.count);
                }
            }
            CloseMenu => self.menu.kind.clear(),
            AcceptMenu => {
                if self.host_effect.is_empty() {
                    let action = match self.menu.kind.as_str() {
                        "work" => Some("work-show"),
                        "history" => Some("switch-conversation"),
                        "archived-history" => Some("restore-conversation"),
                        "models" => Some("select-model"),
                        "efforts" => Some("select-effort"),
                        "config" => Some("toggle-config"),
                        _ => None,
                    };
                    if let Some(action) = action {
                        let selected = self
                            .menu
                            .rows
                            .iter()
                            .filter(|row| row.visible && row.position == self.menu.selection)
                            .collect::<Vec<_>>();
                        if selected.len() > 1 {
                            return Err(effect_conflict());
                        }
                        if let Some(row) = selected.first() {
                            let payload = row.key.clone();
                            if self.menu.kind != "work" {
                                self.menu.kind.clear();
                            }
                            self.effect(action, payload);
                        }
                    }
                }
            }
            ClearReferences => self.references.clear(),
            QueryReferences { query } => {
                self.reference_query = query.clone();
                self.reference_count = 0;
                self.reference_selection = 0;
                self.references_open = true;
            }
            OfferReference {
                name,
                description,
                kind,
                path,
            } => {
                if matches!(kind.as_str(), "file" | "skill" | "hook")
                    && (name
                        .to_lowercase()
                        .contains(&self.reference_query.to_lowercase())
                        || description
                            .to_lowercase()
                            .contains(&self.reference_query.to_lowercase()))
                {
                    self.references.push(Reference {
                        number: self.reference_count,
                        name: name.clone(),
                        description: description.clone(),
                        kind: kind.clone(),
                        path: path.clone(),
                    });
                    self.reference_count += 1;
                    self.references.sort_by_key(|reference| reference.number);
                }
            }
            MoveReference { delta } => {
                if self.reference_count > 0 {
                    self.reference_selection =
                        wrapped(self.reference_selection, *delta, self.reference_count);
                }
            }
            DismissReferences => self.references_open = false,
            ObservePrompt {
                request,
                conversation,
                turn,
                kind,
                cancellation,
            } => {
                if !self.prompts.iter().any(|prompt| {
                    prompt.request == *request
                        && prompt.conversation == *conversation
                        && prompt.live
                }) {
                    self.prompts.push(Prompt {
                        number: self.next_prompt_number,
                        request: request.clone(),
                        conversation: conversation.clone(),
                        turn: turn.clone(),
                        kind: kind.clone(),
                        cancellation: cancellation.clone(),
                        outcome: "answer".into(),
                        status: "waiting".into(),
                        visible: false,
                        live: true,
                        current: 1,
                        questions: Vec::new(),
                    });
                    self.next_prompt_number += 1;
                }
            }
            ObservePromptQuestion {
                number,
                position,
                key,
                header,
                text,
                secret,
                other,
                codec,
                multiple,
                optional,
            } => {
                if let Some(prompt) = self
                    .prompts
                    .iter_mut()
                    .find(|prompt| prompt.number == *number as u64)
                {
                    if !prompt
                        .questions
                        .iter()
                        .any(|question| question.number == *position as u64)
                    {
                        prompt.questions.push(PromptQuestion {
                            number: *position as u64,
                            key: key.clone(),
                            header: header.clone(),
                            text: text.clone(),
                            secret: *secret,
                            other: *other,
                            codec: codec.clone(),
                            multiple: *multiple,
                            optional: *optional,
                            selected: 1,
                            answer: String::new(),
                            answered: false,
                            text_entry: false,
                            problem: String::new(),
                            options: Vec::new(),
                        });
                    }
                }
            }
            ObservePromptOption {
                number,
                position,
                index,
                label,
                description,
                value,
            } => {
                if let Some(question) = self
                    .prompts
                    .iter_mut()
                    .find(|prompt| prompt.number == *number as u64)
                    .and_then(|prompt| {
                        prompt
                            .questions
                            .iter_mut()
                            .find(|question| question.number == *position as u64)
                    })
                {
                    if !question
                        .options
                        .iter()
                        .any(|option| option.number == *index as u64)
                    {
                        question.options.push(PromptOption {
                            number: *index as u64,
                            label: label.clone(),
                            description: description.clone(),
                            value: value.clone(),
                            checked: false,
                        });
                    }
                }
            }
            FocusPrompts => {
                let prompts = self.prompts.clone();
                for prompt in &mut self.prompts {
                    prompt.visible = prompt.live
                        && prompt.conversation == self.active_conversation
                        && prompt.status != "resolved"
                        && !prompts.iter().any(|earlier| {
                            earlier.conversation == prompt.conversation
                                && earlier.number < prompt.number
                                && earlier.status != "resolved"
                                && earlier.live
                        });
                }
            }
            PresentPromptQuestions => {
                for prompt in &mut self.prompts {
                    for question in &mut prompt.questions {
                        question.text_entry = !question.multiple
                            && (question.options.is_empty()
                                || question.other && question.selected == 0);
                    }
                }
            }
            NavigatePrompt { direction } => self.with_question(|question, _| {
                let count = question.options.len() as u64;
                if count > 0 {
                    question.selected = if *direction > 0 {
                        if question.selected >= count {
                            if question.other { 0 } else { 1 }
                        } else {
                            question.selected + 1
                        }
                    } else if question.selected <= if question.other { 0 } else { 1 } {
                        count
                    } else {
                        question.selected - 1
                    };
                }
            }),
            AnswerPromptChoice => self.with_question(|question, current| {
                if !question.multiple {
                    if let Some(option) = question
                        .options
                        .iter()
                        .find(|option| option.number == question.selected)
                    {
                        question.answer = option.value.clone();
                        question.answered = true;
                        *current += 1;
                    }
                }
            }),
            TogglePromptOption => self.with_question(|question, _| {
                if question.multiple {
                    if let Some(option) = question
                        .options
                        .iter_mut()
                        .find(|option| option.number == question.selected)
                    {
                        option.checked = !option.checked;
                    }
                }
            }),
            AnswerPromptMultiple { text } => self.with_question(|question, current| {
                if question.multiple {
                    question.answer = text.clone();
                    question.answered = true;
                    *current += 1;
                }
            }),
            SkipPromptQuestion => self.with_question(|question, current| {
                if question.optional {
                    question.answer = "null".into();
                    question.answered = true;
                    *current += 1;
                }
            }),
            PromptAnswerProblem { problem } => {
                self.with_question(|question, _| question.problem = problem.clone())
            }
            AnswerPromptText { text } => self.with_question(|question, current| {
                if (question.options.is_empty() || question.other && question.selected == 0)
                    && !text.trim().is_empty()
                {
                    question.answer = text.clone();
                    question.answered = true;
                    *current += 1;
                }
            }),
            PreparePromptResponse => {
                if self.connection_state == "connected" && self.host_effect.is_empty() {
                    let candidates = self
                        .prompts
                        .iter()
                        .enumerate()
                        .filter(|(_, prompt)| {
                            prompt.visible
                                && prompt.status == "waiting"
                                && (prompt.outcome == "cancel"
                                    || prompt.questions.iter().all(|question| question.answered))
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    if candidates.len() > 1 {
                        return Err(effect_conflict());
                    }
                    if let Some(index) = candidates.first() {
                        self.prompts[*index].status = "sending".into();
                        self.effect_input_number = self.prompts[*index].number;
                        self.effect("answer-prompt", String::new());
                    }
                }
            }
            CancelPrompt => {
                for prompt in &mut self.prompts {
                    if prompt.visible
                        && prompt.status == "waiting"
                        && !prompt.cancellation.is_empty()
                    {
                        prompt.outcome = "cancel".into();
                    }
                }
            }
            PromptResponseWritten { number, result } => {
                for prompt in &mut self.prompts {
                    if prompt.number == *number as u64 && prompt.status == "sending" {
                        prompt.status = result.clone();
                    }
                }
            }
            ResolvePrompt {
                conversation,
                request,
            } => {
                for prompt in &mut self.prompts {
                    if prompt.live
                        && prompt.conversation == *conversation
                        && prompt.request == *request
                    {
                        prompt.status = "resolved".into();
                    }
                }
            }
            CloseTurnPrompts { conversation, turn } => {
                for prompt in &mut self.prompts {
                    if prompt.conversation == *conversation && prompt.turn == *turn {
                        prompt.status = "resolved".into();
                    }
                }
            }
            ClearResolvedAnswers => {
                for prompt in &mut self.prompts {
                    if prompt.status == "resolved" {
                        for question in &mut prompt.questions {
                            question.answer.clear();
                        }
                    }
                }
            }
            ObserveTurn { conversation, turn } => self.observe_turn(conversation, turn),
            ObserveStoredTurn {
                conversation,
                turn,
                status,
            } => {
                if status == "inProgress" {
                    self.observe_turn(conversation, turn);
                }
            }
            ClearTurn { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    context.active_turn.clear();
                }
            }
            FinishObservedTurn { conversation, turn } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.active_turn == *turn {
                        context.active_turn.clear();
                    }
                }
            }
            ExpireSteering { conversation } => {
                for input in &mut self.pending_inputs {
                    if input.conversation == *conversation && input.status == "waiting to steer" {
                        input.status = "not sent".into();
                    }
                }
            }
            RetainInput { text, mode } => {
                self.pending_inputs.push(PendingInput {
                    number: self.draft_number,
                    conversation: self.active_conversation.clone(),
                    text: text.clone(),
                    status: if self.connection_state != "connected" {
                        "paused"
                    } else {
                        match mode.as_str() {
                            "queue" => "queued",
                            "direct" => "sending",
                            _ => "waiting to steer",
                        }
                    }
                    .into(),
                    client_id: String::new(),
                    attachments: Vec::new(),
                });
                self.draft_number += 1;
            }
            BindInputClientId { number, client } => {
                for input in &mut self.pending_inputs {
                    if input.number == *number as u64 && input.client_id.is_empty() {
                        input.client_id = client.clone();
                    }
                }
            }
            ObserveInputAcceptance {
                conversation,
                client,
            } => {
                if !client.is_empty() {
                    for input in &mut self.pending_inputs {
                        if input.conversation == *conversation && input.client_id == *client {
                            input.status = "accepted".into();
                        }
                    }
                }
            }
            RecordInputAcceptance { number, result } => {
                if result == "accepted" {
                    let inputs = self
                        .pending_inputs
                        .iter()
                        .filter(|input| {
                            input.number == *number as u64
                                && matches!(
                                    input.status.as_str(),
                                    "sending correction" | "submitted"
                                )
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    for input in inputs {
                        self.chat_entry(
                            input.conversation,
                            String::new(),
                            input.client_id,
                            "userMessage".into(),
                            input.text,
                            "completed".into(),
                        );
                    }
                }
            }
            InputReceipt { number, result } => {
                for input in &mut self.pending_inputs {
                    if input.number == *number as u64
                        && matches!(
                            input.status.as_str(),
                            "sending correction" | "sending" | "submitted"
                        )
                    {
                        input.status = result.clone();
                    }
                }
            }
            PrepareSteering | PrepareQueuedInput => {
                let steering = matches!(event, PrepareSteering);
                if self.connection_state == "connected" && self.host_effect.is_empty() {
                    let selected = self
                        .pending_inputs
                        .iter()
                        .enumerate()
                        .find(|(_, input)| {
                            self.input_ready(input, steering)
                                && !self.pending_inputs.iter().any(|earlier| {
                                    earlier.number < input.number
                                        && earlier.conversation == input.conversation
                                        && if steering {
                                            matches!(
                                                earlier.status.as_str(),
                                                "waiting to steer" | "sending correction"
                                            )
                                        } else {
                                            earlier.status == "queued"
                                        }
                                })
                                && !self.pending_inputs.iter().any(|earlier| {
                                    earlier.number < input.number
                                        && self.input_ready(earlier, steering)
                                })
                        })
                        .map(|(index, _)| index);
                    if let Some(index) = selected {
                        let input = &mut self.pending_inputs[index];
                        input.status = if steering {
                            "sending correction"
                        } else {
                            "sending"
                        }
                        .into();
                        self.effect_input_number = input.number;
                        let text = input.text.clone();
                        self.effect(
                            if steering {
                                "send-steering"
                            } else {
                                "submit-queued"
                            },
                            text,
                        );
                    }
                }
            }
            RetainQueuedAttachment { number, attachment } => {
                let attachment = AttachmentIdentity(*attachment as u64);
                let input = self
                    .pending_inputs
                    .iter()
                    .position(|input| input.number == *number as u64);
                let context = self.contexts.iter().position(|context| {
                    context.id == self.active_conversation
                        && context.draft_attachments.contains(&attachment)
                });
                if let (Some(input), Some(context)) = (input, context) {
                    insert_attachment(
                        &mut self.pending_inputs[input].attachments,
                        attachment.clone(),
                    );
                    self.contexts[context]
                        .draft_attachments
                        .retain(|value| *value != attachment);
                }
            }
            SubmitQueued { number } => {
                let input = self
                    .pending_inputs
                    .iter()
                    .position(|input| input.number == *number as u64 && input.status == "sending");
                if let Some(index) = input {
                    let conversation = self.pending_inputs[index].conversation.clone();
                    if let Some(context) = self.context_mut(&conversation) {
                        if context.readiness == TurnReadiness::Ready {
                            context.phase = NorthPhase::Dispatching;
                            context.readiness = TurnReadiness::Active;
                            self.pending_inputs[index].status = "submitted".into();
                        }
                    }
                }
            }
            CopyQueuedToSubmitted { number, attachment } => {
                let attachment = AttachmentIdentity(*attachment as u64);
                if let Some(input) = self.pending_inputs.iter().find(|input| {
                    input.number == *number as u64
                        && input.status == "sending"
                        && input.attachments.contains(&attachment)
                }) {
                    let id = input.conversation.clone();
                    if let Some(context) = self.context_mut(&id) {
                        insert_attachment(&mut context.submitted_attachments, attachment);
                    }
                }
            }
            PrepareInputEdit => {
                if self.host_effect.is_empty() {
                    if let Some(input) = self
                        .pending_inputs
                        .iter_mut()
                        .filter(|input| {
                            input.conversation == self.active_conversation
                                && input_editable(&input.status)
                        })
                        .min_by_key(|input| input.number)
                    {
                        input.status = "editing".into();
                        self.effect_input_number = input.number;
                        let text = input.text.clone();
                        self.effect("edit-pending", text);
                    }
                }
            }
            RestoreInputAttachment { number, attachment } => {
                let attachment = AttachmentIdentity(*attachment as u64);
                if self.pending_inputs.iter().any(|input| {
                    input.number == *number as u64
                        && input.status == "editing"
                        && input.attachments.contains(&attachment)
                }) {
                    if let Some(context) = self.selected_context_mut() {
                        insert_attachment(&mut context.draft_attachments, attachment);
                    }
                }
            }
            ForgetInputAttachment { number, attachment } => {
                for input in &mut self.pending_inputs {
                    if input.number == *number as u64
                        && matches!(input.status.as_str(), "accepted" | "editing")
                    {
                        input
                            .attachments
                            .retain(|value| value.0 != *attachment as u64);
                    }
                }
            }
            ForgetInput { number } => self.pending_inputs.retain(|input| {
                input.number != *number as u64
                    || !matches!(input.status.as_str(), "accepted" | "editing")
            }),
            AppendChat {
                conversation,
                kind,
                text,
            } => self.chat_entry(
                conversation.clone(),
                String::new(),
                String::new(),
                kind.clone(),
                text.clone(),
                "completed".into(),
            ),
            ObserveChatItem {
                conversation,
                turn,
                key,
                kind,
                text,
                status,
                mode,
            } => {
                let indexes = self
                    .chat
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        entry.conversation == *conversation
                            && entry.key == *key
                            && (entry.turn == *turn || kind == "userMessage" && !key.is_empty())
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if indexes.is_empty() {
                    self.chat_entry(
                        conversation.clone(),
                        turn.clone(),
                        key.clone(),
                        kind.clone(),
                        text.clone(),
                        status.clone(),
                    );
                } else {
                    for index in indexes {
                        let entry = &mut std::sync::Arc::make_mut(&mut self.chat)[index];
                        entry.turn = turn.clone();
                        entry.kind = kind.clone();
                        if mode == "append" {
                            entry.text.push_str(text);
                        } else {
                            entry.text = text.clone();
                        }
                        entry.status = status.clone();
                        entry.style = chat_appearance(kind, status).into();
                    }
                }
            }
            ClearChat => {
                if self
                    .chat
                    .iter()
                    .any(|entry| entry.conversation == self.active_conversation)
                {
                    std::sync::Arc::make_mut(&mut self.chat)
                        .retain(|entry| entry.conversation != self.active_conversation);
                }
            }
            Initialize => {
                if !self.initialized {
                    self.initialized = true;
                    self.commands = command_catalog();
                }
            }
            CommandWork { command } => {
                if self.host_effect.is_empty() {
                    let tail = rest_words(command);
                    let action = if tail.trim().is_empty() {
                        "list"
                    } else {
                        first_word(tail)
                    };
                    self.effect(&format!("work-{action}"), rest_words(tail).into());
                }
            }
            Inspect => {}
            ResolveInput { input } => {
                if input.trim().starts_with('/') {
                    self.input_kind = "command".into();
                    if let Some(command) = self
                        .commands
                        .iter()
                        .find(|command| command.name == first_word(input))
                    {
                        self.input_dispatch =
                            format!("command-{}", command.name.trim_start_matches('/'));
                        if command.name == "/q" {
                            self.input_dispatch = "command-quit".into();
                        }
                        self.input_payload = input.trim().into();
                    } else {
                        self.input_dispatch.clear();
                        self.input_payload.clear();
                        self.notice = format!("Unknown command: {}", first_word(input));
                    }
                } else {
                    self.input_dispatch = self.input_handler.clone();
                    self.input_payload = input.clone();
                    self.input_kind = "message".into();
                }
            }
            SelectGoal { goal } => {
                if self.goals.iter().any(|existing| existing.identity == *goal) {
                    self.active_goal = Some(*goal);
                }
            }
            CommandNew { command } => {
                if command == "/new" && self.host_effect.is_empty() {
                    self.effect("new-conversation", String::new());
                }
            }
            CommandResume { command } => {
                if self.host_effect.is_empty() {
                    self.effect("resume-conversation", rest_words(command).into());
                }
            }
            CommandRename { command } => {
                if rest_words(command).is_empty() {
                    self.notice = "Usage: /rename New conversation name".into();
                } else if !self.active_conversation.is_empty() && self.host_effect.is_empty() {
                    self.effect("rename-conversation", rest_words(command).into());
                }
            }
            CommandFork { command } => {
                if command == "/fork"
                    && !self.active_conversation.is_empty()
                    && self.host_effect.is_empty()
                {
                    self.effect("fork-conversation", self.active_conversation.clone());
                }
            }
            CommandArchived { command } => {
                if self.host_effect.is_empty() {
                    self.effect("archived-conversations", rest_words(command).into());
                }
            }
            CommandArchive { command } => {
                if rest_words(command) != "confirm" {
                    self.notice = "Archive this conversation and its related agents? Run /archive confirm. Restore conversations from /archived.".into();
                } else if let Some(context) = self
                    .contexts
                    .iter()
                    .find(|context| context.id == self.active_conversation)
                {
                    if context.readiness != TurnReadiness::Ready {
                        self.notice =
                            "Finish or interrupt the current request before archiving.".into();
                    } else if !self.active_conversation.is_empty() && self.host_effect.is_empty() {
                        self.effect("archive-conversation", self.active_conversation.clone());
                    }
                }
            }
            ObserveArchived { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    context.readiness = TurnReadiness::Paused;
                    context.attached = false;
                }
            }
            ObserveRestored { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    context.readiness = TurnReadiness::Ready;
                }
            }
            HistoryActionCompleted { action } => {
                self.notice = match action.as_str() {
                    "rename" => "Conversation renamed.",
                    "archive" => "Conversation archived. Restore it from /archived.",
                    _ => "Conversation restored.",
                }
                .into()
            }
            CommandModel { command } => {
                if command == "/model" && self.host_effect.is_empty() {
                    self.effect("select-model", String::new());
                }
            }
            CommandEffort { command } => {
                if command == "/effort" && self.host_effect.is_empty() {
                    self.effect("select-effort", String::new());
                }
            }
            CommandConfig { command } => {
                if command == "/config" && self.host_effect.is_empty() {
                    self.effect("open-switchboard", String::new());
                }
            }
            CommandQuit { command } => {
                if command == "/q" && self.host_effect.is_empty() {
                    self.effect("quit", String::new());
                }
            }
            CommandDelegate { command } => {
                if command == "/delegate" && self.input_handler == "submit-input" {
                    self.input_handler = "delegate-task".into();
                    self.notice = "Describe the task to delegate".into();
                }
            }
            DelegateTask { task } => {
                if self.input_handler == "delegate-task" && self.host_effect.is_empty() {
                    self.input_handler = "submit-input".into();
                    self.effect("delegate", task.clone());
                    self.notice.clear();
                }
            }
            SubmitInput { input } => {
                if self.input_handler == "submit-input" && self.host_effect.is_empty() {
                    if let Some(context) = self
                        .contexts
                        .iter()
                        .find(|context| context.id == self.active_conversation)
                    {
                        let action = if self.connection_state != "connected" {
                            "retain-disconnected"
                        } else if context.readiness == TurnReadiness::Active {
                            "steer"
                        } else {
                            "submit"
                        };
                        self.effect(action, input.clone());
                    }
                }
            }
            ClearHostEffect => {
                self.host_effect.clear();
                self.effect_payload.clear();
            }
            CommandGoal { command } => self.command_goal(command),
            GoalTitle { title } => {
                if self.input_handler == "goal-title" {
                    self.input_handler = "goal-objective".into();
                    self.pending_goal_title = title.clone();
                    self.notice = "Describe the desired outcome".into();
                }
            }
            GoalObjective { objective } => {
                if self.input_handler == "goal-objective" {
                    let title = std::mem::take(&mut self.pending_goal_title);
                    self.add_goal(title, objective.clone());
                    self.notice = "Goal created".into();
                }
            }
            GoalEditInput { objective } => {
                if self.input_handler == "goal-edit-input" && !objective.trim().is_empty() {
                    self.edit_goal(objective.clone());
                }
            }
            CommandGoals { command } => {
                if command == "/goals" {
                    self.view("goals");
                    self.notice.clear();
                }
            }
            CommandAgents { command } => {
                if command == "/agents" && self.host_effect.is_empty() {
                    self.effect("select-agent", "{\"limit\":100,\"sortKey\":\"recency_at\",\"sortDirection\":\"desc\",\"sourceKinds\":[\"appServer\",\"cli\",\"vscode\",\"unknown\",\"subAgent\",\"subAgentThreadSpawn\",\"subAgentOther\",\"subAgentReview\",\"subAgentCompact\"],\"useStateDbOnly\":true}".into());
                }
            }
            ShowChat => {
                self.view("chat");
                self.notice.clear();
            }
            ViewChatNext | ViewChatPrevious => {
                if self.active_view == "chat" {
                    self.view("goals");
                }
            }
            ViewGoalsNext | ViewGoalsPrevious => {
                if self.active_view == "goals" {
                    self.view("chat");
                }
            }
            ObserveConversation { conversation } => {
                if !self.known_conversations.contains(conversation) {
                    self.known_conversations.push(conversation.clone());
                }
            }
            AttachImage => {
                let attachment = AttachmentIdentity(self.next_attachment_number);
                if let Some(context) = self.selected_context_mut() {
                    insert_attachment(&mut context.draft_attachments, attachment);
                    self.next_attachment_number += 1;
                }
            }
            DetachImage { attachment } => {
                if let Some(context) = self.selected_context_mut() {
                    context
                        .draft_attachments
                        .retain(|value| value.0 != *attachment as u64);
                }
            }
            CopySubmissionAttachment { attachment } => {
                if let Some(context) = self.selected_context_mut() {
                    let attachment = AttachmentIdentity(*attachment as u64);
                    if context.draft_attachments.contains(&attachment) {
                        insert_attachment(&mut context.submitted_attachments, attachment);
                    }
                }
            }
            ClearSubmittedAttachment {
                conversation,
                attachment,
            } => {
                if let Some(context) = self.context_mut(conversation) {
                    context
                        .submitted_attachments
                        .retain(|value| value.0 != *attachment as u64);
                }
            }
            RequestNewConversation => {
                if self.conversation_change == ConversationChange::Ready
                    && self.pending_conversation.is_empty()
                {
                    self.conversation_change = ConversationChange::Opening;
                }
            }
            SettleNewConversation { conversation } => {
                if self.conversation_change == ConversationChange::Opening
                    && self.pending_conversation.is_empty()
                {
                    self.conversation_change = ConversationChange::Ready;
                    self.active_conversation = conversation.clone();
                    if !self.known_conversations.contains(conversation) {
                        self.known_conversations.push(conversation.clone());
                    }
                }
            }
            FailNewConversation => {
                if self.conversation_change == ConversationChange::Opening
                    && self.pending_conversation.is_empty()
                {
                    self.conversation_change = ConversationChange::Ready;
                }
            }
            RequestSwitchConversation { conversation } => {
                if self.conversation_change == ConversationChange::Ready
                    && self.known_conversations.contains(conversation)
                    && self.pending_conversation.is_empty()
                {
                    self.conversation_change = ConversationChange::Switching;
                    self.pending_conversation = conversation.clone();
                }
            }
            SettleSwitchConversation { conversation } => {
                if self.conversation_change == ConversationChange::Switching
                    && self.pending_conversation == *conversation
                {
                    self.conversation_change = ConversationChange::Ready;
                    self.active_conversation = conversation.clone();
                    self.pending_conversation.clear();
                }
            }
            FailSwitchConversation { conversation } => {
                if self.conversation_change == ConversationChange::Switching
                    && self.pending_conversation == *conversation
                {
                    self.conversation_change = ConversationChange::Ready;
                    self.pending_conversation.clear();
                }
            }
            Submit | Delegate => {
                if let Some(context) = self.selected_context_mut() {
                    if context.readiness == TurnReadiness::Ready {
                        context.phase = if matches!(event, Submit) {
                            NorthPhase::Dispatching
                        } else {
                            NorthPhase::Delegating
                        };
                        context.readiness = TurnReadiness::Active;
                        context.terminal_child = None;
                        self.draft_number += 1;
                    }
                }
            }
            ChildSpawned {
                conversation,
                child,
            } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.phase == NorthPhase::Delegating && context.active_child.is_none() {
                        context.phase = NorthPhase::Settling;
                        context.active_child = nonempty(child);
                    }
                }
            }
            SettleSuccess { conversation } | SettleFailure { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.phase == NorthPhase::Dispatching
                        && context.readiness == TurnReadiness::Active
                    {
                        context.phase = if matches!(event, SettleSuccess { .. }) {
                            NorthPhase::Completed
                        } else {
                            NorthPhase::Failed
                        };
                        context.readiness = TurnReadiness::Ready;
                    }
                }
            }
            AbandonTurn { conversation } | SettleInterrupted { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.readiness == TurnReadiness::Active {
                        context.phase = if matches!(event, AbandonTurn { .. }) {
                            NorthPhase::Failed
                        } else {
                            NorthPhase::Interrupted
                        };
                        context.readiness = TurnReadiness::Ready;
                        context.terminal_child = context.active_child.take();
                    }
                }
            }
            SettleDelegationSuccess {
                conversation,
                child,
            }
            | FailDelegationAfterChild {
                conversation,
                child,
            } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.phase == NorthPhase::Settling
                        && context.active_child.as_deref() == Some(child.as_str())
                        && context.terminal_child.is_none()
                        && context.readiness == TurnReadiness::Active
                    {
                        context.phase = if matches!(event, SettleDelegationSuccess { .. }) {
                            NorthPhase::Completed
                        } else {
                            NorthPhase::Failed
                        };
                        context.active_child = None;
                        context.terminal_child = nonempty(child);
                        context.readiness = TurnReadiness::Ready;
                    }
                }
            }
            FailDelegationBeforeChild { conversation } => {
                if let Some(context) = self.context_mut(conversation) {
                    if context.phase == NorthPhase::Delegating
                        && context.readiness == TurnReadiness::Active
                    {
                        context.phase = NorthPhase::Failed;
                        context.readiness = TurnReadiness::Ready;
                    }
                }
            }
        }
        Ok(())
    }

    fn input_ready(&self, input: &PendingInput, steering: bool) -> bool {
        input.status
            == if steering {
                "waiting to steer"
            } else {
                "queued"
            }
            && self.contexts.iter().any(|context| {
                context.id == input.conversation
                    && if steering {
                        !context.active_turn.is_empty()
                    } else {
                        context.readiness == TurnReadiness::Ready
                            && !matches!(
                                context.phase,
                                NorthPhase::Failed | NorthPhase::Interrupted
                            )
                    }
            })
    }

    fn add_usage(
        &mut self,
        key: &str,
        owner: &str,
        label: &str,
        value: i64,
        known: bool,
        unit: &str,
    ) {
        self.usage_rows.push(StoredUsageRow {
            key: key.into(),
            owner: owner.into(),
            visible: false,
            row: UsageRow {
                label: label.into(),
                value: value as f64,
                known,
                unit: unit.into(),
                order: self.usage_next_row,
            },
        });
        self.usage_next_row += 1;
    }

    fn with_question(&mut self, mut operation: impl FnMut(&mut PromptQuestion, &mut u64)) {
        for prompt in &mut self.prompts {
            if prompt.visible && prompt.status == "waiting" {
                if let Some(question) = prompt
                    .questions
                    .iter_mut()
                    .find(|question| question.number == prompt.current)
                {
                    operation(question, &mut prompt.current);
                }
            }
        }
    }

    fn observe_turn(&mut self, conversation: &str, turn: &str) {
        if let Some(context) = self.context_mut(conversation) {
            context.active_turn = turn.into();
            if context.readiness == TurnReadiness::Ready {
                context.phase = NorthPhase::Dispatching;
            }
            context.readiness = TurnReadiness::Active;
        }
    }
    fn view(&mut self, view: &str) {
        self.active_view = view.into();
        self.next_view_handler = format!("view-{view}-next");
        self.previous_view_handler = format!("view-{view}-previous");
    }
    fn add_goal(&mut self, title: String, objective: String) {
        let identity = self.next_goal_order;
        self.goals.push(Goal {
            identity,
            title,
            objective,
            status: "active".into(),
            order: identity,
            prior_objectives: Vec::new(),
        });
        self.active_goal = Some(identity);
        self.next_goal_order += 1;
        self.input_handler = "submit-input".into();
    }
    fn edit_goal(&mut self, objective: String) {
        if let Some(goal) = self.selected_goal_mut() {
            if goal.status == "active" {
                goal.prior_objectives
                    .push(std::mem::replace(&mut goal.objective, objective));
                self.input_handler = "submit-input".into();
                self.notice = "Goal updated".into();
            }
        }
    }
    fn command_goal(&mut self, command: &str) {
        let tail = rest_words(command);
        let action = match first_word(tail) {
            "edit" => "edit",
            "clear" => "clear",
            _ => "set",
        };
        let text = if matches!(first_word(tail), "set" | "edit" | "clear") {
            rest_words(tail)
        } else {
            tail
        }
        .trim();
        match action {
            "set" => {
                if text.is_empty() {
                    if self.input_handler == "submit-input" {
                        self.input_handler = "goal-title".into();
                        self.pending_goal_title.clear();
                        self.notice = "Name the Goal".into();
                    }
                } else {
                    self.add_goal(text.into(), text.into());
                    self.notice = "Goal set".into();
                }
            }
            "edit" => {
                if self.active_goal.is_none() {
                    self.notice = "No selected goal".into();
                } else if text.is_empty() {
                    if self.host_effect.is_empty() {
                        let objective = self
                            .selected_goal_mut()
                            .map(|goal| goal.objective.clone())
                            .unwrap_or_default();
                        self.input_handler = "goal-edit-input".into();
                        self.notice = "Edit the desired outcome".into();
                        self.effect("edit-draft", objective);
                    }
                } else {
                    self.edit_goal(text.into());
                }
            }
            "clear" => {
                if self.active_goal.is_none() {
                    self.notice = "No selected goal".into();
                } else if !text.is_empty() {
                    self.notice = "Usage: /goal clear".into();
                } else {
                    if let Some(goal) = self.selected_goal_mut() {
                        goal.status = "cleared".into();
                    }
                    self.active_goal = None;
                    self.input_handler = "submit-input".into();
                    self.notice = "Goal cleared".into();
                }
            }
            _ => unreachable!(),
        }
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.into())
}
fn effect_conflict() -> NorthError {
    NorthError::State("More than one host effect requested in a step".into())
}
fn wrapped(selection: i64, delta: i64, count: i64) -> i64 {
    let next = selection + delta;
    if next < 0 {
        count - 1
    } else if next >= count {
        0
    } else {
        next
    }
}
fn menu_matches(row: &MenuRow, query: &str) -> bool {
    format!("{} {} {}", row.label, row.description, row.key)
        .to_lowercase()
        .contains(query)
}
fn insert_attachment(values: &mut Vec<AttachmentIdentity>, value: AttachmentIdentity) {
    if !values.contains(&value) {
        values.push(value);
        values.sort_by_key(|value| value.0);
    }
}
fn input_editable(status: &str) -> bool {
    matches!(
        status,
        "queued" | "paused" | "not sent" | "waiting to steer"
    )
}
fn first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or_default()
}
fn rest_words(text: &str) -> &str {
    let text = text.trim_start();
    text.find(char::is_whitespace)
        .map(|index| text[index..].trim_start())
        .unwrap_or_default()
}
fn chat_appearance<'a>(kind: &'a str, status: &str) -> &'a str {
    match kind {
        "userMessage" | "operator" => "operator",
        "agentMessage" => "markdown",
        "commandExecution" => match status {
            "completed" => "command-success",
            "inProgress" => "command-running",
            _ => "command-failure",
        },
        "fileChange" | "diff" => "diff",
        other => other,
    }
}

fn command_catalog() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            name: "/new".into(),
            description: "start a new conversation".into(),
            order: 1,
        },
        CommandSpec {
            name: "/resume".into(),
            description: "switch to a previous conversation".into(),
            order: 2,
        },
        CommandSpec {
            name: "/model".into(),
            description: "choose the active model and reasoning effort".into(),
            order: 3,
        },
        CommandSpec {
            name: "/effort".into(),
            description: "change reasoning effort for the active model".into(),
            order: 4,
        },
        CommandSpec {
            name: "/config".into(),
            description: "toggle the context switchboard".into(),
            order: 5,
        },
        CommandSpec {
            name: "/delegate".into(),
            description: "delegate one explicit task".into(),
            order: 6,
        },
        CommandSpec {
            name: "/goal".into(),
            description: "set, edit, or clear the selected goal".into(),
            order: 7,
        },
        CommandSpec {
            name: "/goals".into(),
            description: "inspect Goals".into(),
            order: 9,
        },
        CommandSpec {
            name: "/agents".into(),
            description: "change selected agent".into(),
            order: 10,
        },
        CommandSpec {
            name: "/q".into(),
            description: "quit North".into(),
            order: 12,
        },
        CommandSpec {
            name: "/rename".into(),
            description: "rename the selected conversation".into(),
            order: 13,
        },
        CommandSpec {
            name: "/fork".into(),
            description: "continue a copy of this conversation".into(),
            order: 14,
        },
        CommandSpec {
            name: "/archive".into(),
            description: "archive this conversation and its related agents".into(),
            order: 15,
        },
        CommandSpec {
            name: "/archived".into(),
            description: "find and restore archived conversations".into(),
            order: 16,
        },
        CommandSpec {
            name: "/usage".into(),
            description: "inspect context, limits, and reset availability".into(),
            order: 17,
        },
        CommandSpec {
            name: "/compact".into(),
            description: "make more room in this conversation".into(),
            order: 18,
        },
        CommandSpec {
            name: "/reconnect".into(),
            description: "restore the connection without resending messages".into(),
            order: 19,
        },
        CommandSpec {
            name: "/work".into(),
            description: "list, search, open and edit work items".into(),
            order: 20,
        },
    ]
}

mod chat_snapshot {
    use super::ChatEntry;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    pub fn serialize<S: serde::Serializer>(
        chat: &Arc<Vec<ChatEntry>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        chat.as_ref().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Arc<Vec<ChatEntry>>, D::Error> {
        Vec::deserialize(deserializer).map(Arc::new)
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

impl AttachmentIdentity {
    pub const fn number(self) -> u64 {
        self.0
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

impl CommandSpec {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
}
