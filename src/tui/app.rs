use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Text};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::codex::{Codex, ConversationSnapshot};
use crate::command_surface::{Picker, menu_direction};
use crate::composer::{Composer, ImageHandles, Submission};
use crate::error::{NorthError, NorthResult};
use crate::north_state::{AttachmentIdentity, NorthPhase, NorthState};
use crate::{
    agent_catalog, codex, command_surface, foundation, local_store, north_state, prompts,
    references, rpc, usage, work_items,
};


pub(super) struct App {
    pub(super) cwd: PathBuf,
    pub(super) branch: String,
    pub(super) state: NorthState,
    pub(super) dispatched_revision: Option<north_state::StateRevisionId>,
    pub(super) codex: Option<Codex>,
    pub(super) connection: Option<rpc::Rpc>,
    pub(super) events: Option<rpc::Events>,
    pub(super) model: String,
    pub(super) reasoning_effort: String,
    pub(super) composer: Composer,
    pub(super) transcript: Vec<(Speaker, String)>,
    pub(super) status: String,
    pub(super) turns: BTreeMap<String, RunningTurn>,
    pub(super) editors: BTreeMap<String, Composer>,
    pub(super) pending_images: BTreeMap<u64, ImageHandles>,
    pub(super) submission_scope: tempfile::TempDir,
    pub(super) steering: BTreeMap<u64, JoinHandle<NorthResult<()>>>,
    pub(super) prompt_responses: BTreeMap<u64, JoinHandle<NorthResult<()>>>,
    pub(super) request_errors: Vec<JoinHandle<NorthResult<()>>>,
    pub(super) prompt_editor: tui_textarea::TextArea<'static>,
    pub(super) prompt_editor_key: Option<(u64, u64)>,
    pub(super) prompt_editors: BTreeMap<(u64, u64), tui_textarea::TextArea<'static>>,
    pub(super) prompt_scroll: u16,
    pub(super) transcript_search: Option<tui_textarea::TextArea<'static>>,
    pub(super) menu_editor: tui_textarea::TextArea<'static>,
    pub(super) usage_task: Option<JoinHandle<NorthResult<serde_json::Value>>>,
    pub(super) reconnect_task: Option<JoinHandle<NorthResult<Reconnection>>>,
    pub(super) reconciliation: Option<Reconnection>,
    pub(super) picker: Option<Picker>,
    pub(super) settings_model: Option<String>,
    pub(super) slash_menu: command_surface::SlashMenu,
    pub(super) reference_candidates: Option<Vec<references::Reference>>,
    pub(super) reference_observation: Option<String>,
    pub(super) draft_last_saved: Instant,
    pub(super) transcript_revision: u64,
    pub(super) rendered_transcript: Option<(u64, usize, Text<'static>)>,
    pub(super) rendered_transcript_viewport: Option<TranscriptViewport>,
}

pub(super) struct TranscriptViewport {
    pub(super) revision: u64,
    pub(super) offset: u64,
    pub(super) status: Vec<Line<'static>>,
    pub(super) limit: u16,
    pub(super) buffer: ratatui::buffer::Buffer,
}

pub(super) struct RunningTurn {
    pub(super) task: JoinHandle<TurnCompletion>,
    pub(super) input: Option<u64>,
    pub(super) interrupt: Option<oneshot::Sender<()>>,
    pub(super) started: Instant,
}

pub(super) struct Reconnection {
    pub(super) codex: Codex,
    pub(super) events: rpc::Events,
    pub(super) conversations: VecDeque<NorthResult<codex::ResumedConversation>>,
}

pub(super) struct TurnCompletion {
    pub(super) result: TurnResult,
}

pub(super) enum TurnResult {
    Direct(NorthResult<codex::TurnOutcome>),
    Compaction(NorthResult<()>),
    Delegation {
        child_id: Option<String>,
        result: NorthResult<codex::DelegationOutcome>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum Speaker {
    Operator,
    North,
    CommandSuccess,
    CommandFailure,
    CommandRunning,
    FileChange,
    Notice,
    System,
}

impl App {
    pub(super) fn open(cwd: PathBuf) -> NorthResult<Self> {
        Self::open_stored(cwd, &local_store::LocalStore::default_root()?)
    }

    #[cfg(test)]
    pub(super) fn ephemeral(cwd: PathBuf) -> NorthResult<Self> {
        Self::with_state(cwd, NorthState::open()?)
    }

    pub(super) fn open_stored(cwd: PathBuf, root: &Path) -> NorthResult<Self> {
        let cwd = cwd.canonicalize()?;
        let state = NorthState::open_stored(Some(local_store::LocalStore::open(&root.join("rust-state-v1"), &cwd)?))?;
        let mut app = Self::with_state(cwd, state)?;
        for context in app.state.conversations() {
            let images = context.draft_attachments.iter().map(|identity|
                app.state.image_file(*identity).map(|file| (*identity, file))
            ).collect::<NorthResult<Vec<_>>>()?;
            let mut editor = Composer::new();
            editor.restore_saved(&context.saved_draft, images);
            app.editors.insert(context.id.clone(), editor);
        }
        for input in app.state.pending_inputs() {
            let images = input.attachments.iter().map(|identity|
                app.state.image_file(*identity).map(|file| (*identity, file))
            ).collect::<NorthResult<Vec<_>>>()?;
            app.pending_images.insert(input.number, ImageHandles::restored(images));
        }
        let selected = app.state.active_conversation().unwrap_or_default();
        if let Some(editor) = app.editors.remove(selected) { app.composer = editor; }
        app.project_chat();
        Ok(app)
    }

    pub(super) fn with_state(cwd: PathBuf, state: NorthState) -> NorthResult<Self> {
        let branch = session_branch(&cwd);
        let mut menu_editor = tui_textarea::TextArea::default();
        menu_editor.insert_str(&state.menu().query);
        Ok(Self {
            cwd,
            branch,
            state,
            dispatched_revision: None,
            codex: None,
            connection: None,
            events: None,
            model: "Codex default".into(),
            reasoning_effort: "default".into(),
            composer: Composer::new(),
            transcript: Vec::new(),
            status: "idle".into(),
            turns: BTreeMap::new(),
            editors: BTreeMap::new(),
            pending_images: BTreeMap::new(),
            submission_scope: tempfile::Builder::new().prefix("north-input-").tempdir()?,
            steering: BTreeMap::new(),
            prompt_responses: BTreeMap::new(),
            request_errors: Vec::new(),
            prompt_editor: tui_textarea::TextArea::default(),
            prompt_editor_key: None,
            prompt_editors: BTreeMap::new(),
            prompt_scroll: 0,
            transcript_search: None,
            menu_editor,
            usage_task: None,
            reconnect_task: None,
            reconciliation: None,
            picker: None,
            settings_model: None,
            slash_menu: command_surface::SlashMenu::default(),
            reference_candidates: None,
            reference_observation: None,
            draft_last_saved: Instant::now(),
            transcript_revision: 0,
            rendered_transcript: None,
            rendered_transcript_viewport: None,
        })
    }

    pub(super) fn save_composer(&mut self) -> NorthResult<()> {
        if self.draft_last_saved.elapsed() < Duration::from_millis(250) {
            return Ok(());
        }
        self.save_composer_now()
    }

    pub(super) fn save_composer_now(&mut self) -> NorthResult<()> {
        let text = self.composer.text();
        if self.state.conversation(self.state.active_conversation().unwrap_or_default())
            .is_some_and(|context| context.saved_draft != text) {
            self.state.save_draft(&text)?;
        }
        self.draft_last_saved = Instant::now();
        Ok(())
    }

    pub(super) async fn accept_submission(&mut self, mut submission: Submission) -> bool {
        let input = submission.text.clone();
        let previous_notice = self.state.notice().to_owned();
        let mut settings_model = None;
        let accepted = self.state.checkpoint_after(|state| {
            state.accept_draft_input(&input)?;
            let effect = state.host_effect();
            if let Some(effect) = effect.as_ref() {
                match effect.action() {
                    "open-switchboard" => Self::open_switchboard(state)?,
                    "select-model" => Self::open_model_picker(state, self.codex.as_ref(), &self.model)?,
                    "select-effort" => settings_model = Some(Self::open_effort_picker(
                        state, self.codex.as_ref(), &self.model, &self.reasoning_effort)?),
                    "quit" => {},
                    _ => state.clear_host_effect()?,
                }
            }
            Ok(effect)
        });
        let effect = match accepted {
            Ok(effect) => effect,
            Err(error) => {
                self.composer.restore_submission(submission);
                self.record_error(error);
                return false;
            }
        };
        self.draft_last_saved = Instant::now();
        let notice = self.state.notice();
        if notice != previous_notice && !notice.is_empty() {
            self.record_chat(Speaker::Notice, notice.to_owned());
        }
        let Some(effect) = effect else {
            self.detach_images(submission.attachment_identities());
            return false;
        };
        let action = effect.action().to_owned();
        let payload = effect.payload().to_owned();
        match action.as_str() {
            action if action.starts_with("work-") => {
                self.detach_images(submission.attachment_identities());
                if let Err(error) = self.show_work(action, &payload).await { self.record_error(error); }
                false
            }
            "quit" => true,
            "edit-draft" => {
                self.detach_images(submission.attachment_identities());
                let removed = self.composer.replace_text(&payload);
                self.detach_images(removed);
                false
            }
            "new-conversation" => {
                self.detach_images(submission.attachment_identities());
                self.new_conversation().await;
                false
            }
            "resume-conversation" | "select-agent" | "archived-conversations" => {
                self.detach_images(submission.attachment_identities());
                self.open_conversation_picker(&payload, action == "select-agent", action == "archived-conversations").await;
                false
            }
            "rename-conversation" | "fork-conversation" | "archive-conversation" => {
                self.detach_images(submission.attachment_identities());
                if let Err(error) = self.manage_history(&action, &payload).await { self.record_error(error); }
                false
            }
            "read-usage" => {
                self.detach_images(submission.attachment_identities());
                if self.usage_task.is_none() && let Some(connection) = self.connection.clone() {
                    self.usage_task = Some(tokio::spawn(async move {
                        connection.request("account/rateLimits/read", serde_json::json!({})).await
                    }));
                }
                false
            }
            "compact-conversation" => {
                self.detach_images(submission.attachment_identities());
                match self.turn_session_in(&payload) {
                    Ok(mut session) => {
                        let (interrupt, signal) = oneshot::channel();
                        self.turns.insert(payload, RunningTurn {
                            task: tokio::spawn(async move { TurnCompletion { result: TurnResult::Compaction(session.compact(signal).await) } }),
                            input: None, interrupt: Some(interrupt), started: Instant::now(),
                        });
                    }
                    Err(error) => {
                        let _ = self.state.settle_direct_in(&payload, false);
                        self.record_error(error);
                    }
                }
                false
            }
            "select-model" | "select-effort" | "open-switchboard" => {
                self.detach_images(submission.attachment_identities());
                if let Some(model) = settings_model { self.settings_model = Some(model); }
                self.menu_editor = tui_textarea::TextArea::default();
                false
            }
            "submit" => {
                submission.text = payload;
                self.submit_direct(submission);
                false
            }
            "steer" => {
                submission.text = payload;
                self.retain_input(submission, false);
                false
            }
            "retain-disconnected" => {
                submission.text = payload;
                self.retain_input(submission, true);
                false
            }
            "delegate" => {
                submission.text = payload;
                self.submit_delegation(submission);
                false
            }
            unknown => {
                self.detach_images(submission.attachment_identities());
                self.record_error(NorthError::Protocol(format!(
                    "North projected unknown host effect {unknown}"
                )));
                false
            }
        }
    }

    pub(super) fn submit_direct(&mut self, submission: Submission) {
        let text = submission.text.clone();
        let number = match self.state.retain_direct(&text) {
            Ok(number) => number,
            Err(error) => {
                self.composer.restore_submission(submission);
                self.record_error(error);
                return;
            }
        };
        self.pending_images.insert(number, submission.into_images());
        let image_paths = match self.pending_image_paths(number) {
            Ok(paths) => paths,
            Err(error) => {
                self.record_input_receipt(number, "not sent");
                self.record_error(error);
                return;
            }
        };
        if let Err(error) = self.state.submit_queued(number) {
            self.record_input_receipt(number, "not sent");
            self.record_error(error);
            return;
        }
        let conversation = self.state.active_conversation().unwrap_or_default().to_owned();
        self.launch_direct(&conversation, text, image_paths, number);
    }

    pub(super) fn turn_session(&self) -> NorthResult<codex::TurnSession> {
        let conversation = self.state.active_conversation()
            .ok_or_else(|| NorthError::Protocol("No conversation selected".into()))?;
        self.turn_session_in(conversation)
    }

    pub(super) fn turn_session_in(&self, conversation: &str) -> NorthResult<codex::TurnSession> {
        let connection = self.connection.clone()
            .ok_or_else(|| NorthError::Protocol("Codex connection is unavailable".into()))?;
        let context = self.state.conversation(conversation)
            .ok_or_else(|| NorthError::State("No conversation to run".into()))?;
        Ok(codex::TurnSession::new(connection, conversation.into(), context.model.clone()))
    }

    pub(super) fn input_client_id(&mut self, number: u64) -> NorthResult<String> {
        let id = format!("{}/{number}", self.submission_scope.path().display());
        self.state.bind_input_client_id(number, &id)?;
        Ok(id)
    }

    pub(super) fn launch_direct(&mut self, conversation: &str, text: String, image_paths: Vec<PathBuf>, input: u64) {
        let client_id = match self.input_client_id(input) {
            Ok(id) => id,
            Err(error) => { self.record_error_in(conversation, error); return; }
        };
        let mut session = match self.turn_session_in(conversation) {
            Ok(session) => session,
            Err(error) => {
                self.record_input_receipt(input, "not sent");
                let _ = self.state.settle_direct_in(conversation, false);
                self.record_error_in(conversation, error);
                return;
            }
        };
        self.status = "working".into();
        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = session
                .run_turn_interruptible(&text, &image_paths, Some(&client_id), interrupt_rx)
                .await;
            TurnCompletion {
                result: TurnResult::Direct(result),
            }
        });
        self.turns.insert(conversation.into(), RunningTurn {
            task, input: Some(input), interrupt: Some(interrupt_tx), started: Instant::now(),
        });
    }

    pub(super) fn retain_input(&mut self, submission: Submission, queued: bool) {
        let result = if queued {
            self.state.queue_input(&submission.text)
        } else {
            self.state.retain_steering(&submission.text)
        };
        match result {
            Ok(number) => { self.pending_images.insert(number, submission.into_images()); }
            Err(error) => {
                self.composer.restore_submission(submission);
                self.record_error(error);
            }
        }
    }

    pub(super) async fn handle_composer_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter if !self.composer.is_empty() => {
                let submission = self.composer.take_submission();
                return self.accept_submission(submission).await;
            }
            KeyCode::Tab if !self.composer.is_empty() => {
                let submission = self.composer.take_submission();
                self.retain_input(submission, true);
            }
            KeyCode::Enter => {}
            _ => {
                self.slash_menu.selected = 0;
                let removed = self.composer.handle_key(key);
                self.detach_images(removed);
            }
        }
        false
    }

    pub(super) fn pending_image_paths(&self, number: u64) -> NorthResult<Vec<PathBuf>> {
        let input = self.state.pending_inputs().iter().find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Pending input is missing".into()))?;
        self.pending_images.get(&number)
            .ok_or_else(|| NorthError::Protocol("Pending input image handles are missing".into()))?
            .image_paths(&input.attachments).map_err(NorthError::Protocol)
    }

    pub(super) fn record_input_receipt(&mut self, number: u64, receipt: &str) {
        if let Err(error) = self.state.input_receipt(number, receipt) {
            self.record_error(error);
        } else {
            self.project_chat();
        }
    }

    pub(super) fn forget_input(&mut self, number: u64) {
        match self.state.forget_input(number) {
            Ok(()) => { self.pending_images.remove(&number); }
            Err(error) => self.record_error(error),
        }
    }

    pub(super) fn edit_pending_input(&mut self) -> NorthResult<()> {
        if !self.composer.is_empty() {
            self.record_chat(Speaker::Notice, "Clear the draft before editing a queued message.".into());
            return Ok(());
        }
        let Some(number) = self.state.prepare_input_edit()? else { return Ok(()); };
        let text = self.state.host_effect().unwrap().payload().to_owned();
        self.pending_image_paths(number)?;
        self.state.restore_input(number)?;
        self.state.clear_host_effect()?;
        let images = self.pending_images.remove(&number)
            .ok_or_else(|| NorthError::Protocol("Pending input image handles are missing".into()))?;
        self.composer.restore_submission(images.with_text(text));
        Ok(())
    }

    pub(super) fn dispatch_pending_input(&mut self) -> NorthResult<()> {
        if let Some(number) = self.state.prepare_steering()? {
            let text = self.state.host_effect().unwrap().payload().to_owned();
            self.state.clear_host_effect()?;
            let images = match self.pending_image_paths(number) {
                Ok(images) => images,
                Err(error) => { self.record_input_receipt(number, "not sent"); return Err(error); }
            };
            let Some(connection) = self.connection.clone() else {
                self.record_input_receipt(number, "not sent");
                return Ok(());
            };
            let thread = self.state.pending_inputs().iter().find(|input| input.number == number)
                .ok_or_else(|| NorthError::State("Correction has no conversation".into()))?.conversation.clone();
            let turn = self.state.conversation(&thread).ok_or_else(|| NorthError::State("Correction has no active conversation".into()))?.active_turn.clone();
            let client_id = self.input_client_id(number)?;
            self.steering.insert(number, tokio::spawn(async move {
                codex::steer_turn(&connection, &thread, &turn, &text, &images, &client_id).await
            }));
        }
        if let Some(number) = self.state.prepare_queued_input()? {
            let text = self.state.host_effect().unwrap().payload().to_owned();
            self.state.clear_host_effect()?;
            let images = match self.pending_image_paths(number) {
                Ok(images) => images,
                Err(error) => { self.record_input_receipt(number, "not sent"); return Err(error); }
            };
            let conversation = self.state.pending_inputs().iter().find(|input| input.number == number)
                .ok_or_else(|| NorthError::State("Queued message has no conversation".into()))?.conversation.clone();
            self.state.submit_queued(number)?;
            self.launch_direct(&conversation, text, images, number);
        }
        Ok(())
    }

    pub(super) fn dispatch_ready_work(&mut self) -> NorthResult<()> {
        if self.dispatched_revision != self.state.revision() {
            if !self.state.prompts().is_empty() { self.dispatch_prompt_response()?; }
            if !self.state.pending_inputs().is_empty() { self.dispatch_pending_input()?; }
            self.dispatched_revision = self.state.revision();
        }
        Ok(())
    }

    pub(super) async fn collect_steering(&mut self) {
        let finished = self.steering.iter().filter_map(|(&number, task)| task.is_finished().then_some(number))
            .collect::<Vec<_>>();
        for number in finished {
            let result = self.steering.remove(&number).unwrap().await;
            let result = result.unwrap_or_else(|error| Err(NorthError::Protocol(error.to_string())));
            match result {
                Ok(()) => {
                    self.record_input_receipt(number, "accepted");
                    self.forget_input(number);
                }
                Err(error) => {
                    self.record_input_receipt(number, if matches!(error, NorthError::Rejected(_)) { "not sent" } else { "delivery unknown" });
                    self.record_chat(Speaker::Notice, format!("Correction #{number} was not confirmed. Its text and images have been kept."));
                }
            }
        }
    }

    pub(super) fn submit_delegation(&mut self, submission: Submission) {
        let attachments = match self.state.delegate() {
            Ok(attachments) => attachments,
            Err(error) => {
                self.record_error(error);
                return;
            }
        };
        let image_paths = match submission.image_paths(&attachments) {
            Ok(paths) => paths,
            Err(error) => {
                self.settle_delegation_failure();
                self.record_error(NorthError::Protocol(error));
                return;
            }
        };
        let mut session = match self.turn_session() {
            Ok(session) => session,
            Err(error) => {
                self.settle_delegation_failure();
                self.record_error(error);
                return;
            }
        };
        self.status = "working".into();
        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut child_id = None;
            let result = session
                .run_delegate_interruptible(
                    &submission.text,
                    &image_paths,
                    |spawned| {
                        child_id = Some(spawned.to_owned());
                        Ok(())
                    },
                    interrupt_rx,
                )
                .await;
            drop(submission);
            TurnCompletion {
                result: TurnResult::Delegation { child_id, result },
            }
        });
        let conversation = self.state.active_conversation().unwrap_or_default().to_owned();
        self.turns.insert(conversation, RunningTurn {
            task, input: None, interrupt: Some(interrupt_tx), started: Instant::now(),
        });
    }

    pub(super) fn detach_images(&mut self, identities: Vec<AttachmentIdentity>) {
        for identity in identities {
            if let Err(error) = self.state.detach_image(identity) {
                self.record_error(error);
            }
        }
    }

    pub(super) fn is_working(&self) -> bool {
        self.turns.contains_key(self.state.active_conversation().unwrap_or_default())
    }

    pub(super) fn record_chat(&mut self, speaker: Speaker, text: String) {
        let kind = match speaker {
            Speaker::Operator => "operator",
            Speaker::North => "agentMessage",
            Speaker::CommandSuccess => "command-success",
            Speaker::CommandFailure => "command-failure",
            Speaker::CommandRunning => "command-running",
            Speaker::FileChange => "fileChange",
            Speaker::Notice => "notice",
            Speaker::System => "error",
        };
        if let Err(error) = self.state.append_chat(kind, &text) {
            self.record_error(error);
            return;
        }
        self.project_chat();
    }

    pub(super) fn project_chat(&mut self) {
        self.transcript = self.state.chat().iter()
            .filter(|entry| entry.visible)
            .map(|entry| {
                let speaker = match entry.style.as_str() {
                    "operator" => Speaker::Operator,
                    "markdown" => Speaker::North,
                    "command-success" => Speaker::CommandSuccess,
                    "command-failure" => Speaker::CommandFailure,
                    "command-running" => Speaker::CommandRunning,
                    "diff" => Speaker::FileChange,
                    "error" => Speaker::System,
                    _ => Speaker::Notice,
                };
                (speaker, entry.text.clone())
            }).collect();
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.rendered_transcript = None;
    }

    pub(super) fn append_transcript_error(&mut self, message: String) {
        self.transcript.push((Speaker::System, message));
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.rendered_transcript = None;
    }

    pub(super) fn collect_events(&mut self, budget: Option<Duration>) -> bool {
        let started = Instant::now();
        let mut processed = false;
        loop {
            if processed && budget.is_some_and(|budget| started.elapsed() >= budget) {
                return true;
            }
            let Some(events) = self.events.as_mut() else { break; };
            let message = match events.try_recv() {
                Ok(Ok(message)) => message,
                Ok(Err(error)) => {
                    self.events = None;
                    self.connection = None;
                    if let Err(error) = self.state.connection_lost() { self.record_error(error); }
                    self.record_error(NorthError::Protocol(error));
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => {
                    self.events = None;
                    self.connection = None;
                    if let Err(error) = self.state.connection_lost() { self.record_error(error); }
                    self.record_error(NorthError::Protocol(format!("Event stream needs reconciliation: {error}")));
                    break;
                }
            };
            processed = true;
            if message["method"] == "serverRequest/resolved" {
                if let (Some(conversation), Some(request)) = (message["params"]["threadId"].as_str(), message["params"].get("requestId")) {
                    if let Err(error) = self.state.resolve_prompt(conversation, &request.to_string()) { self.record_error(error); }
                }
            } else {
                match prompts::decode_request(&message, self.state.chat()) {
                    Ok(Some(prompt)) => {
                        if let Err(error) = self.state.observe_prompt(&prompt) { self.record_error(error); }
                    }
                    Err(_) => self.reject_server_request(&message, -32602, "North could not read this interaction request."),
                    Ok(None) => {
                        if message.get("id").is_some() && message.get("method").is_some() {
                            self.reject_server_request(&message, -32601, "North does not support this interaction request.");
                        }
                    }
                }
            }
            match message["method"].as_str() {
                Some("thread/tokenUsage/updated") => {
                    let result = serde_json::from_value::<usage::ContextUpdate>(message["params"].clone())
                        .map_err(NorthError::from).and_then(|update| self.state.observe_context_usage(&update));
                    if let Err(error) = result { self.record_error(error); }
                }
                Some("account/rateLimits/updated") => {
                    let result = serde_json::from_value::<usage::AccountUsage>(message["params"].clone())
                        .map_err(NorthError::from).and_then(|usage| self.state.observe_account_usage(&usage, foundation::clock::Timestamp::now().unix_seconds() as u64));
                    if let Err(error) = result { self.record_error(error); }
                }
                Some("thread/archived" | "thread/unarchived") => {
                    if let Some(conversation) = message["params"]["threadId"].as_str() {
                        let result = if message["method"] == "thread/archived" {
                            self.state.observe_archived(conversation)
                        } else { self.state.observe_restored(conversation) };
                        if let Err(error) = result { self.record_error(error); }
                    }
                }
                Some(method @ ("turn/started" | "turn/completed")) => {
                    if let (Some(conversation), Some(turn)) = (
                        message["params"]["threadId"].as_str(), message["params"]["turn"]["id"].as_str(),
                    ) {
                        let result = if method == "turn/started" {
                            self.state.observe_turn(conversation, turn)
                        } else {
                            self.state.finish_observed_turn(conversation, turn)
                        };
                        if let Err(error) = result { self.record_error(error); }
                    }
                }
                _ => {}
            }
            if let Some(conversation) = message["params"]["threadId"].as_str() {
                let items = if message["method"] == "turn/completed" {
                    message["params"]["turn"]["items"].as_array().map(Vec::as_slice).unwrap_or_default()
                } else { std::slice::from_ref(&message["params"]["item"]) };
                for client in codex::accepted_user_inputs(items) {
                    if let Err(error) = self.state.observe_input_acceptance(conversation, &client) { self.record_error(error); }
                }
            }
            let updates = codex::chat_updates(&message);
            if !updates.is_empty() {
                let observations = updates.iter().map(|item| north_state::ChatEntryInput {
                    conversation: &item.conversation, turn: &item.turn, key: &item.key,
                    kind: &item.kind, text: &item.text, status: &item.status, append: item.append,
                }).collect::<Vec<_>>();
                if let Err(error) = self.state.observe_chat_items(&observations) {
                    self.record_error(error);
                    return false;
                }
                self.project_chat();
            }
        }
        false
    }

    pub(super) fn sync_prompt_editor(&mut self) {
        let key = self.state.active_prompt().map(|prompt| (prompt.number, prompt.current));
        if key != self.prompt_editor_key {
            if let Some(prior) = self.prompt_editor_key {
                self.prompt_editors.insert(prior, std::mem::take(&mut self.prompt_editor));
            }
            self.prompt_editor = key.and_then(|key| self.prompt_editors.remove(&key)).unwrap_or_default();
            self.prompt_editor_key = key;
            self.prompt_scroll = 0;
        }
        self.prompt_editors.retain(|(number, question), _| self.state.prompts().iter().any(|prompt|
            prompt.number == *number && prompt.status != "resolved"
                && prompt.questions.iter().any(|item| item.number == *question && !item.answered)));
    }

    pub(super) fn reject_server_request(&mut self, request: &serde_json::Value, code: i64, description: &str) {
        let Some(connection) = self.connection.clone() else { return; };
        let response = serde_json::json!({"id":request["id"],"error":{"code":code,"message":description}});
        self.request_errors.push(tokio::spawn(async move { connection.send(response).await }));
        self.record_chat(Speaker::Notice, description.into());
    }

    pub(super) fn handle_prompt_key(&mut self, key: KeyEvent) -> NorthResult<bool> {
        if self.picker.is_some() || !self.state.menu().kind.is_empty() { return Ok(false); }
        self.sync_prompt_editor();
        let Some(prompt) = self.state.active_prompt() else { return Ok(false); };
        if key.code == KeyCode::PageUp {
            self.prompt_scroll = self.prompt_scroll.saturating_sub(8);
            return Ok(true);
        }
        if key.code == KeyCode::PageDown {
            self.prompt_scroll = self.prompt_scroll.saturating_add(8);
            return Ok(true);
        }
        if prompt.status != "waiting" { return Ok(true); }
        let question = prompts::current_question(prompt);
        let text_entry = question.is_some_and(|question| question.text_entry);
        let multiple = question.is_some_and(|question| question.multiple);
        if key.code == KeyCode::Esc {
            self.state.cancel_prompt()?;
        } else if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state.skip_prompt_question()?;
        } else if key.code == KeyCode::Char(' ') && multiple {
            self.state.toggle_prompt_option()?;
        } else if key.code == KeyCode::Enter {
            if let Some(question) = question.filter(|_| text_entry || multiple) {
                let answer = if multiple { prompts::multiple_answer(question) }
                    else { prompts::text_answer(question, &self.prompt_editor.lines().join("\n")) };
                match answer {
                    Ok(value) if multiple => self.state.answer_prompt_multiple(&value)?,
                    Ok(value) => self.state.answer_prompt_text(&value)?,
                    Err(problem) => self.state.prompt_answer_problem(&problem)?,
                }
            } else {
                self.state.answer_prompt_choice()?;
            }
        } else if let Some(direction) = menu_direction(&key) {
            self.state.navigate_prompt(direction as i32)?;
        } else if key.code == KeyCode::Up {
            self.state.navigate_prompt(-1)?;
        } else if key.code == KeyCode::Down || key.code == KeyCode::Tab {
            self.state.navigate_prompt(1)?;
        } else if text_entry {
            self.prompt_editor.input(key);
        }
        self.sync_prompt_editor();
        Ok(true)
    }

    pub(super) fn dispatch_prompt_response(&mut self) -> NorthResult<()> {
        let Some(number) = self.state.prepare_prompt_response()? else { return Ok(()); };
        self.state.clear_host_effect()?;
        let prompt = self.state.prompts().iter().find(|prompt| prompt.number == number)
            .ok_or_else(|| NorthError::Protocol("Prompt response is missing".into()))?;
        let message = prompts::response(prompt)?;
        let Some(connection) = self.connection.clone() else {
            self.state.prompt_response_written(number, false)?;
            return Ok(());
        };
        self.prompt_responses.insert(number, tokio::spawn(async move { connection.send(message).await }));
        Ok(())
    }

    pub(super) async fn collect_prompt_responses(&mut self) {
        for index in (0..self.request_errors.len()).rev() {
            if self.request_errors[index].is_finished() {
                match self.request_errors.swap_remove(index).await {
                    Ok(Ok(())) => {}
                    _ => self.record_chat(Speaker::Notice, "The reply could not be delivered. Reconnect to continue.".into()),
                }
            }
        }
        let finished = self.prompt_responses.iter().filter_map(|(&number, task)| task.is_finished().then_some(number))
            .collect::<Vec<_>>();
        for number in finished {
            let delivered = matches!(self.prompt_responses.remove(&number).unwrap().await, Ok(Ok(())));
            if let Err(error) = self.state.prompt_response_written(number, delivered) { self.record_error(error); }
        }
        self.sync_prompt_editor();
    }

    pub(super) async fn collect_finished_turn(&mut self) {
        let finished = self.turns.iter().filter_map(|(id, turn)| turn.task.is_finished().then_some(id.clone())).collect::<Vec<_>>();
        if finished.is_empty() { return; }
        self.collect_events(None);
        for conversation in finished {
            let turn = self.turns.remove(&conversation).unwrap();
            let completion = match turn.task.await {
                Ok(completion) => completion,
                Err(error) => {
                    if let Some(number) = turn.input { self.record_input_receipt(number, "delivery unknown"); }
                    if let Err(error) = self.state.abandon_turn_in(&conversation) { self.record_error_in(&conversation, error); }
                    self.record_error_in(&conversation, NorthError::Protocol(format!("Background turn stopped: {error}")));
                    continue;
                }
            };
            if let Some(number) = turn.input {
                if matches!(completion.result, TurnResult::Direct(Ok(_)) | TurnResult::Direct(Err(NorthError::Interrupted))) {
                    self.record_input_receipt(number, "accepted");
                }
                if self.state.pending_inputs().iter().any(|input| input.number == number && input.status == "accepted") {
                    self.forget_input(number);
                } else {
                    let rejected = matches!(completion.result, TurnResult::Direct(Err(NorthError::Rejected(_))));
                    self.record_input_receipt(number, if rejected { "not sent" } else { "delivery unknown" });
                }
            }
            let (settled, error) = match completion.result {
                TurnResult::Direct(Err(NorthError::Interrupted)) | TurnResult::Compaction(Err(NorthError::Interrupted)) => (
                    self.state.settle_interrupted_in(&conversation, None), Some(NorthError::Interrupted),
                ),
                TurnResult::Direct(result) => (self.state.settle_direct_in(&conversation, result.is_ok()), result.err()),
                TurnResult::Compaction(result) => {
                    let settled = self.state.settle_direct_in(&conversation, result.is_ok());
                    if result.is_ok() && settled.is_ok() {
                        if let Err(error) = self.state.compaction_completed() { self.record_error(error); }
                        let text = self.state.notice().to_owned();
                        self.record_chat_in(&conversation, "notice", &text);
                    }
                    (settled, result.err())
                }
                TurnResult::Delegation { child_id, result: Err(NorthError::Interrupted) } => (
                    self.state.settle_interrupted_in(&conversation, child_id.as_deref()), Some(NorthError::Interrupted),
                ),
                TurnResult::Delegation { child_id, result } => (
                    self.state.settle_delegation_in(&conversation, child_id.as_deref(), result.is_ok()), result.err(),
                ),
            };
            if let Err(error) = settled { self.record_error_in(&conversation, error); }
            match error {
                Some(NorthError::Interrupted) => {
                    self.record_chat_in(&conversation, "notice", "Interrupted");
                    if self.state.active_conversation() == Some(conversation.as_str()) { self.status = "idle".into(); }
                }
                Some(error) => self.record_error_in(&conversation, error),
                None if self.state.active_conversation() == Some(conversation.as_str()) => self.status = "complete".into(),
                None => {}
            }
        }
    }

    pub(super) fn interrupt_turn(&mut self) {
        let conversation = self.state.active_conversation().unwrap_or_default();
        if let Some(interrupt) = self.turns.get_mut(conversation).and_then(|turn| turn.interrupt.take()) {
            let _ = interrupt.send(());
        }
    }

    pub(super) fn record_chat_in(&mut self, conversation: &str, kind: &str, text: &str) {
        if let Err(error) = self.state.append_chat_in(conversation, kind, text) {
            self.append_transcript_error(error.user_message());
        } else { self.project_chat(); }
    }

    pub(super) fn record_error_in(&mut self, conversation: &str, error: NorthError) {
        if self.state.active_conversation() == Some(conversation) { self.status = "failed".into(); }
        self.record_chat_in(conversation, "error", &error.user_message());
    }

    pub(super) async fn ensure_codex(&mut self, requested_conversation: Option<&str>) -> NorthResult<()> {
        if self.reconciliation.is_some() { self.collect_reconciliation(None)?; }
        if self.codex.is_none() {
            if self.state.connection_state() == "disconnected" && self.state.attached_conversations().next().is_none() {
                self.state.finish_reconnect(true)?;
            }
            if self.state.connection_state() == "disconnected" {
                self.collect_reconnect().await?;
                if requested_conversation.is_none() { return Ok(()); }
                if let Some(task) = self.reconnect_task.take() {
                    let result = task.await.map_err(|error| NorthError::Protocol(format!("Reconnection stopped: {error}")))?;
                    self.finish_reconnection(result, None).await?;
                }
                if self.codex.is_none() { return Err(NorthError::Configuration("Could not reopen the saved conversations; workspace data has been retained".into())); }
                if let Some(conversation) = requested_conversation {
                    self.try_switch_conversation(conversation, true).await?;
                }
                return Ok(());
            }
            let mut codex = Codex::connect(&self.cwd).await?;
            let connection = codex.connection();
            self.events = Some(connection.subscribe());
            self.connection = Some(connection);
            let selected_conversation = if let Some(conversation) = requested_conversation {
                self.state.observe_conversation(conversation)?;
                Some(conversation.to_owned())
            } else {
                let conversations = codex.conversations(&self.cwd, false).await?;
                for conversation in &conversations {
                    self.state.observe_conversation(&conversation.id)?;
                }
                conversations.first().map(|conversation| conversation.id.clone())
            };
            if let Some(conversation) = selected_conversation {
                self.state.request_switch_conversation(&conversation)?;
                let snapshot = match codex.resume_conversation(&conversation).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        self.state.fail_switch_conversation(&conversation)?;
                        return Err(error);
                    }
                };
                self.state.settle_switch_conversation(&conversation)?;
                self.attach_conversation(snapshot);
            } else {
                self.state.request_new_conversation()?;
                let thread_id = match codex.start_new_conversation(&self.cwd).await {
                    Ok(thread_id) => thread_id,
                    Err(error) => {
                        self.state.fail_new_conversation()?;
                        return Err(error);
                    }
                };
                self.state.settle_new_conversation(&thread_id)?;
            }
            self.model = codex.model().to_owned();
            self.reasoning_effort = codex.reasoning_effort().to_owned();
            let conversation = self.state.active_conversation().unwrap_or_default().to_owned();
            self.state.observe_settings(&conversation, &self.model, &self.reasoning_effort)?;
            self.codex = Some(codex);
        }
        Ok(())
    }

    pub(super) async fn new_conversation(&mut self) {
        let previous = self.state.active_conversation().unwrap_or_default().to_owned();
        if let Err(error) = self.state.save_draft(&self.composer.text()) { self.record_error(error); return; }
        if let Err(error) = self.state.request_new_conversation() {
            self.record_error(error);
            return;
        }
        let Some(codex) = self.codex.as_mut() else {
            let _ = self.state.fail_new_conversation();
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        match codex.start_new_conversation(&self.cwd).await {
            Ok(thread_id) => match self.state.settle_new_conversation(&thread_id) {
                Ok(()) => {
                    self.model = codex.model().to_owned();
                    self.reasoning_effort = codex.reasoning_effort().to_owned();
                    if let Err(error) = self.state.observe_settings(&thread_id, &self.model, &self.reasoning_effort) { self.record_error(error); return; }
                    self.focus_conversation(&previous);
                    self.status = "idle".into();
                    self.project_chat();
                }
                Err(error) => self.record_error(error),
            },
            Err(error) => {
                let _ = self.state.fail_new_conversation();
                self.record_error(error);
            }
        }
    }

    pub(super) async fn open_conversation_picker(&mut self, query: &str, agents: bool, archived: bool) {
        let Some(codex) = self.codex.as_mut() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        let conversations = if !agents {
            codex.conversations(&self.cwd, archived).await
        } else {
            match serde_json::from_str(query) {
                Ok(parameters) => codex.list_conversations(parameters).await,
                Err(error) => Err(error.into()),
            }
        };
        match conversations {
            Ok(conversations) => {
                for conversation in &conversations {
                    if let Err(error) = self.state.observe_conversation(&conversation.id) {
                        self.record_error(error);
                        return;
                    }
                }
                let query = if agents { "" } else { query };
                if let Err(error) = self.state.open_history(&conversations, query, archived) {
                    self.record_error(error);
                    return;
                }
                self.menu_editor = tui_textarea::TextArea::default();
                self.menu_editor.insert_str(&self.state.menu().query);
                self.picker = None;
            }
            Err(error) => self.record_error(error),
        }
    }

    pub(super) async fn collect_usage(&mut self) {
        if !self.usage_task.as_ref().is_some_and(|task| task.is_finished()) { return; }
        let result = match self.usage_task.take().unwrap().await {
            Ok(Ok(value)) => serde_json::from_value::<usage::AccountUsage>(value).map_err(NorthError::from)
                .and_then(|usage| self.state.observe_account_usage(&usage, foundation::clock::Timestamp::now().unix_seconds() as u64)),
            Ok(Err(error)) => Err(error),
            Err(error) => Err(NorthError::Protocol(format!("Usage request stopped: {error}"))),
        };
        if let Err(error) = result { self.record_error(error); }
    }

    pub(super) async fn collect_reconnect(&mut self) -> NorthResult<()> {
        if self.reconciliation.is_some() {
            return self.collect_reconciliation(Some(Duration::from_millis(8)));
        }
        if self.state.connection_state() == "disconnected" && self.reconnect_task.is_none() {
            self.state.begin_reconnect()?;
            let conversations = self.state.attached_conversations().map(str::to_owned).collect::<Vec<_>>();
            let cwd = self.cwd.clone();
            let old = self.codex.take();
            self.reconnect_task = Some(tokio::spawn(async move {
                if let Some(old) = old { let _ = old.shutdown().await; }
                let mut codex = Codex::connect(&cwd).await?;
                let events = codex.connection().subscribe();
                let mut resumed = VecDeque::new();
                for conversation in conversations { resumed.push_back(codex.resume_conversation(&conversation).await); }
                Ok(Reconnection { codex, events, conversations: resumed })
            }));
        }
        if !self.reconnect_task.as_ref().is_some_and(|task| task.is_finished()) { return Ok(()); }
        let result = self.reconnect_task.take().unwrap().await
            .map_err(|error| NorthError::Protocol(format!("Reconnection stopped: {error}")))?;
        self.finish_reconnection(result, Some(Duration::from_millis(8))).await
    }

    pub(super) async fn finish_reconnection(&mut self, result: NorthResult<Reconnection>, budget: Option<Duration>) -> NorthResult<()> {
        match result {
            Ok(reconnected) => {
                self.settle_disconnected_tasks().await;
                self.reconciliation = Some(reconnected);
                self.collect_reconciliation(budget)?;
            }
            Err(error) => {
                self.state.finish_reconnect(false)?;
                self.record_error(error);
                self.record_chat(Speaker::Notice, self.state.notice().to_owned());
            }
        }
        Ok(())
    }

    pub(super) fn collect_reconciliation(&mut self, budget: Option<Duration>) -> NorthResult<()> {
        let started = Instant::now();
        while let Some(result) = self.reconciliation.as_mut().and_then(|pending| pending.conversations.pop_front()) {
            match result {
                Ok(resumed) => self.reconcile_conversation(resumed)?,
                Err(error) => self.record_error(error),
            }
            if budget.is_some_and(|budget| started.elapsed() >= budget) { return Ok(()); }
        }
        let Some(reconnected) = self.reconciliation.take() else { return Ok(()); };
        // Live events follow all resumed snapshots so older snapshots cannot
        // overwrite an event that was delivered during reconnection.
        self.connection = Some(reconnected.codex.connection());
        self.events = Some(reconnected.events);
        self.codex = Some(reconnected.codex);
        self.state.finish_reconnect(true)?;
        if let Some(selected) = self.state.active_conversation().and_then(|id| self.state.conversation(id)) {
            self.model = selected.model.clone();
            self.reasoning_effort = selected.effort.clone();
            if let Some(codex) = self.codex.as_mut() {
                codex.select_attached_conversation(&selected.id, &selected.model, &selected.effort);
            }
        }
        self.record_chat(Speaker::Notice, self.state.notice().to_owned());
        Ok(())
    }

    pub(super) async fn settle_disconnected_tasks(&mut self) {
        for (_, turn) in std::mem::take(&mut self.turns) {
            turn.task.abort();
            let _ = turn.task.await;
        }
        for (_, task) in std::mem::take(&mut self.steering) { task.abort(); let _ = task.await; }
        for (_, task) in std::mem::take(&mut self.prompt_responses) { task.abort(); let _ = task.await; }
        for task in self.request_errors.drain(..) { task.abort(); let _ = task.await; }
    }

    pub(super) fn reconcile_conversation(&mut self, resumed: codex::ResumedConversation) -> NorthResult<()> {
        let conversation = resumed.snapshot.id.clone();
        self.state.begin_conversation_reconciliation(&conversation)?;
        self.merge_conversation(resumed.snapshot)?;
        self.watch_resumed_turn(&conversation, resumed.session);
        let accepted = self.state.pending_inputs().iter()
            .filter(|input| input.conversation == conversation && input.status == "accepted")
            .map(|input| input.number).collect::<Vec<_>>();
        for number in accepted { self.forget_input(number); }
        Ok(())
    }

    pub(super) async fn manage_history(&mut self, action: &str, payload: &str) -> NorthResult<()> {
        let previous = self.state.active_conversation().unwrap_or_default().to_owned();
        let codex = self.codex.as_mut().ok_or_else(|| NorthError::Protocol("Codex client is unavailable".into()))?;
        match action {
            "rename-conversation" => {
                codex.rename_conversation(&previous, payload).await?;
                self.state.history_action_completed("rename")?;
            }
            "fork-conversation" => {
                self.state.save_draft(&self.composer.text())?;
                self.state.request_new_conversation()?;
                let fork = match codex.fork_conversation(payload).await {
                    Ok(fork) => fork,
                    Err(error) => { self.state.fail_new_conversation()?; return Err(error); }
                };
                self.state.settle_new_conversation(&fork.snapshot.id)?;
                self.focus_conversation(&previous);
                self.attach_conversation(fork);
            }
            "archive-conversation" => {
                codex.archive_conversation(payload).await?;
                self.state.observe_archived(payload)?;
                self.state.history_action_completed("archive")?;
                self.new_conversation().await;
            }
            "restore-conversation" => {
                codex.restore_conversation(payload).await?;
                self.state.observe_restored(payload)?;
                self.state.history_action_completed("restore")?;
                self.switch_conversation(payload).await;
            }
            _ => return Err(NorthError::State(format!("Unknown history action {action}"))),
        }
        let notice = self.state.notice().to_owned();
        if !notice.is_empty() { self.record_chat(Speaker::Notice, notice); }
        Ok(())
    }

    pub(super) async fn switch_conversation(&mut self, conversation_id: &str) {
        if let Err(error) = self.try_switch_conversation(conversation_id, false).await {
            self.record_error(error);
        }
    }

    pub(super) async fn try_switch_conversation(&mut self, conversation_id: &str, confirm_remote: bool) -> NorthResult<()> {
        if !confirm_remote && self.state.active_conversation() == Some(conversation_id) {
            return Ok(());
        }
        let previous = self.state.active_conversation().unwrap_or_default().to_owned();
        self.state.save_draft(&self.composer.text())?;
        if confirm_remote { self.state.observe_conversation(conversation_id)?; }
        self.state.request_switch_conversation(conversation_id)?;
        if let Some(context) = self.state.conversation(conversation_id).filter(|context| context.attached && !confirm_remote) {
            if let Some(codex) = self.codex.as_mut() {
                codex.select_attached_conversation(conversation_id, &context.model, &context.effort);
            }
            self.state.settle_switch_conversation(conversation_id)?;
            self.focus_conversation(&previous);
            return Ok(());
        }
        let Some(codex) = self.codex.as_mut() else {
            self.state.fail_switch_conversation(conversation_id)?;
            return Err(NorthError::Protocol("Codex client is unavailable".into()));
        };
        match codex.resume_conversation(conversation_id).await {
            Ok(snapshot) => {
                self.state.settle_switch_conversation(conversation_id)?;
                self.focus_conversation(&previous);
                self.attach_conversation(snapshot);
                Ok(())
            }
            Err(error) => {
                self.state.fail_switch_conversation(conversation_id)?;
                Err(error)
            }
        }
    }

    pub(super) async fn handle_menu_key(&mut self, key: KeyEvent) -> NorthResult<bool> {
        if self.state.menu().kind.is_empty() { return Ok(false); }
        if let Some(delta) = menu_direction(&key) {
            self.state.move_menu(delta)?;
        } else if key.code == KeyCode::Esc {
            self.state.close_menu()?;
        } else if key.code == KeyCode::Enter {
            self.state.accept_menu()?;
            if let Some(effect) = self.state.host_effect() {
                self.state.clear_host_effect()?;
                match effect.action() {
                    action if action.starts_with("work-") => self.show_work(action, effect.payload()).await?,
                    "switch-conversation" => self.switch_conversation(effect.payload()).await,
                    "restore-conversation" => self.manage_history(effect.action(), effect.payload()).await?,
                    "select-model" => self.select_model_menu(effect.payload()).await,
                    "select-effort" => {
                        let model = self.settings_model.clone().unwrap_or_else(|| self.model.clone());
                        let effort = effect.payload().to_owned();
                        self.apply_model_selection(&model, &effort).await;
                    }
                    "toggle-config" => self.toggle_config_menu(effect.payload()),
                    action => return Err(NorthError::State(format!("Unknown menu effect {action}"))),
                }
            }
        } else {
            self.menu_editor.input(key);
            self.state.query_menu(&self.menu_editor.lines().join("\n"))?;
        }
        Ok(true)
    }

    pub(super) async fn show_work(&mut self, action: &str, payload: &str) -> NorthResult<()> {
        let view = work_items::execute(action, payload).await?;
        self.state.clear_host_effect()?;
        work_items::project(&mut self.state, &view)?;
        self.menu_editor = tui_textarea::TextArea::default();
        Ok(())
    }

    pub(super) fn attach_conversation(&mut self, resumed: codex::ResumedConversation) {
        let conversation = resumed.snapshot.id.clone();
        self.load_conversation(resumed.snapshot);
        self.watch_resumed_turn(&conversation, resumed.session);
    }

    pub(super) fn watch_resumed_turn(&mut self, conversation: &str, mut session: codex::TurnSession) {
        let Some(context) = self.state.conversation(&conversation) else { return; };
        if context.active_turn.is_empty() || self.turns.contains_key(conversation) { return; }
        let turn = context.active_turn.clone();
        let (interrupt, signal) = oneshot::channel();
        self.turns.insert(conversation.into(), RunningTurn {
            task: tokio::spawn(async move {
                TurnCompletion { result: TurnResult::Direct(session.wait_for_turn(&turn, signal).await) }
            }),
            input: None,
            interrupt: Some(interrupt),
            started: Instant::now(),
        });
    }

    pub(super) fn load_conversation(&mut self, snapshot: ConversationSnapshot) {
        if let Err(error) = self.state.clear_chat() { self.record_error(error); return; }
        if let Err(error) = self.merge_conversation(snapshot) { self.record_error(error); }
    }

    pub(super) fn merge_conversation(&mut self, snapshot: ConversationSnapshot) -> NorthResult<()> {
        self.state.observe_snapshot(&snapshot)?;
        if self.state.active_conversation() == Some(snapshot.id.as_str()) {
            self.model = snapshot.model;
            self.reasoning_effort = snapshot.reasoning_effort;
        }
        self.project_chat();
        Ok(())
    }

    pub(super) fn focus_conversation(&mut self, previous: &str) {
        let selected = self.state.active_conversation().unwrap_or_default().to_owned();
        if selected != previous {
            let mut editor = self.editors.remove(&selected).unwrap_or_else(Composer::new);
            if editor.is_empty() && let Some(context) = self.state.conversation(&selected) {
                editor.insert_text(&context.saved_draft);
            }
            let previous_editor = std::mem::replace(&mut self.composer, editor);
            self.editors.insert(previous.into(), previous_editor);
        }
        if let Some(context) = self.state.conversation(&selected) {
            self.model = context.model.clone();
            self.reasoning_effort = context.effort.clone();
            self.status = context.phase.label().into();
        }
        self.slash_menu.selected = 0;
        self.transcript_search = None;
        self.reference_observation = None;
        self.project_chat();
        self.sync_prompt_editor();
    }

    pub(super) fn handle_transcript_key(&mut self, key: KeyEvent) -> NorthResult<bool> {
        if let Some(editor) = self.transcript_search.as_mut() {
            match key.code {
                KeyCode::Esc => { self.transcript_search = None; }
                KeyCode::Enter => {
                    self.state.search_transcript(&editor.lines().join("\n"))?;
                    self.transcript_search = None;
                    self.project_chat();
                }
                _ => { editor.input(key); }
            }
            return Ok(true);
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('f') {
            let mut editor = tui_textarea::TextArea::default();
            if let Some(context) = self.state.conversation(self.state.active_conversation().unwrap_or_default()) {
                editor.insert_str(&context.transcript_query);
            }
            self.transcript_search = Some(editor);
        } else if control && key.code == KeyCode::Char('d') {
            self.state.toggle_changes()?;
            self.project_chat();
        } else if control && key.code == KeyCode::Char('y') {
            let text = self.displayed_messages();
            match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
                Ok(()) => self.status = "Copied displayed messages".into(),
                Err(_) => self.record_chat(Speaker::Notice, "Could not copy the displayed messages.".into()),
            }
        } else if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            self.state.scroll_transcript(if key.code == KeyCode::PageUp { 12.0 } else { -12.0 })?;
        } else if control && matches!(key.code, KeyCode::Home | KeyCode::End) {
            self.state.scroll_transcript(if key.code == KeyCode::Home { u16::MAX as f64 } else { -(u16::MAX as f64) })?;
        } else { return Ok(false); }
        Ok(true)
    }

    pub(super) fn displayed_messages(&self) -> String {
        self.transcript.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join("\n\n")
    }

    pub(super) fn open_model_picker(state: &mut NorthState, codex: Option<&Codex>, current_model: &str) -> NorthResult<()> {
        let codex = codex.ok_or_else(|| NorthError::Protocol("Codex client is unavailable".into()))?;
        let rows = codex.models().iter().map(|model| {
            (model.model.as_str(), model.model.as_str(), model.description.as_str(),
                if model.model == current_model { "current" } else if model.is_default { "default" } else { "" })
        }).collect::<Vec<_>>();
        if rows.is_empty() { return Err(NorthError::Protocol("Codex returned no selectable models".into())); }
        state.finish_settings_effect("models", "Select Model and Effort", &rows)
    }

    pub(super) fn open_effort_picker(state: &mut NorthState, codex: Option<&Codex>, current_model: &str, current_effort: &str) -> NorthResult<String> {
        let codex = codex.ok_or_else(|| NorthError::Protocol("Codex client is unavailable".into()))?;
        let model = codex.models().iter().find(|model| model.model == current_model).ok_or_else(||
            NorthError::Protocol(format!(
                "active model {} is absent from the Codex catalog",
                current_model
            )))?;
        let rows = model.reasoning.iter().map(|option| {
            (option.effort.as_str(), effort_label_ref(&option.effort), option.description.as_str(),
                if option.effort == current_effort { "current" } else if option.effort == model.default_effort { "default" } else { "" })
        }).collect::<Vec<_>>();
        state.finish_settings_effect("efforts", &format!("Select Reasoning Level for {}", model.model), &rows)?;
        Ok(model.model.clone())
    }

    pub(super) async fn select_model_menu(&mut self, model_name: &str) {
        let Some(codex) = self.codex.as_ref() else { self.record_error(NorthError::Protocol("Codex client is unavailable".into())); return; };
        let Some(model) = codex.models().iter().find(|model| model.model == model_name) else { self.record_error(NorthError::State(format!("Unknown model {model_name}"))); return; };
        let rows = model.reasoning.iter().map(|option| {
            (option.effort.as_str(), effort_label_ref(&option.effort), option.description.as_str(),
                if model.model == self.model && option.effort == self.reasoning_effort { "current" } else if option.effort == model.default_effort { "default" } else { "" })
        }).collect::<Vec<_>>();
        self.settings_model = Some(model.model.clone());
        if let Err(error) = self.state.open_settings_menu("efforts", &format!("Select Reasoning Level for {}", model.model), &rows) { self.record_error(error); }
        self.menu_editor = tui_textarea::TextArea::default();
    }

    pub(super) fn toggle_config_menu(&mut self, id: &str) {
        let active = agent_catalog::activation_units().ok().and_then(|units| units.into_iter().find(|unit| unit.id == id).map(|unit| !unit.active)).unwrap_or(true);
        match agent_catalog::toggle_activation_unit(id, active) {
            Ok(units) => {
                let rows = units.iter().map(|unit| (unit.id.as_str(), unit.id.as_str(), unit.description.as_str(), if unit.active { "on" } else { "off" })).collect::<Vec<_>>();
                if let Err(error) = self.state.open_settings_menu("config", "Context Switchboard", &rows) { self.record_error(error); }
                self.menu_editor = tui_textarea::TextArea::default();
                self.reference_candidates = None;
            }
            Err(error) => self.record_error(error),
        }
    }

    pub(super) fn open_switchboard(state: &mut NorthState) -> NorthResult<()> {
        let units = agent_catalog::activation_units()?;
        let rows = units.iter().map(|unit| (unit.id.as_str(), unit.id.as_str(), unit.description.as_str(), if unit.active { "on" } else { "off" })).collect::<Vec<_>>();
        state.finish_settings_effect("config", "Context Switchboard", &rows)
    }

    pub(super) fn toggle_switchboard_selection(&mut self) {
        let Some(Picker::Switchboard { units, index }) = self.picker.as_ref() else {
            return;
        };
        let Some(selected) = units.get(*index) else {
            return;
        };
        let id = selected.id.clone();
        let active = !selected.active;
        match agent_catalog::toggle_activation_unit(&id, active) {
            Ok(units) => {
                self.reference_candidates = None;
                self.reference_observation = None;
                let index = units.iter().position(|unit| unit.id == id).unwrap_or(0);
                self.picker = Some(Picker::Switchboard { units, index });
            }
            Err(error) => self.record_error(error),
        }
    }

    pub(super) fn reference_switchboard_selection(&mut self) {
        let Some(Picker::Switchboard { units, index }) = self.picker.as_ref() else {
            return;
        };
        let Some(unit) = units.get(*index) else {
            return;
        };
        self.composer
            .insert_reference(&unit.id, &unit.kind, &unit.source, false);
        self.picker = None;
        self.slash_menu.selected = 0;
    }

    pub(super) fn reference_query(&self) -> Option<String> {
        let query = self.composer.reference_query()?;
        (self.state.references_open() && self.state.reference_query() == query).then_some(query)
    }

    pub(super) fn refresh_reference_menu(&mut self) {
        if self.picker.is_some() || !self.state.menu().kind.is_empty() || self.state.active_prompt().is_some() || self.transcript_search.is_some() { return; }
        let query = self.composer.reference_query();
        if query.is_none() {
            self.reference_candidates = None;
            self.reference_observation = None;
            return;
        }
        if self.reference_candidates.is_none() {
            let mut candidates = Vec::new();
            match agent_catalog::activation_units() {
                Ok(units) => candidates.extend(units.into_iter().map(Into::into)),
                Err(error) => self.record_error(error),
            }
            match references::project_files(&self.cwd) {
                Ok(files) => candidates.extend(files),
                Err(error) => self.record_error(error),
            }
            self.reference_candidates = Some(candidates);
        }
        if query != self.reference_observation {
            let result = self.state.query_references(query.as_deref().unwrap_or_default(), self.reference_candidates.as_deref().unwrap_or_default());
            self.reference_observation = query;
            if let Err(error) = result { self.record_error(error); }
        }
    }

    pub(super) fn handle_reference_key(&mut self, key: &KeyEvent) -> bool {
        if self.reference_query().is_none() {
            return false;
        }
        if key.code == KeyCode::Esc {
            if let Err(error) = self.state.dismiss_references() { self.record_error(error); }
            return true;
        }
        if self.state.references().is_empty() {
            return false;
        }
        if let Some(delta) = menu_direction(key) {
            if let Err(error) = self.state.move_reference(delta) { self.record_error(error); }
            return true;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Tab) {
            let Some(reference) = self.state.references().get(self.state.reference_selection()) else { return false; };
            self.composer
                .insert_reference(&reference.name, &reference.kind, &reference.path, true);
            if let Err(error) = self.state.dismiss_references() { self.record_error(error); }
            return true;
        }
        false
    }

    pub(super) async fn accept_picker_selection(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        match picker {
            Picker::Switchboard { units, index } => {
                self.picker = Some(Picker::Switchboard { units, index });
            }
            Picker::Models { models, index } => {
                self.picker = Some(Picker::efforts(
                    models,
                    index,
                    &self.model,
                    &self.reasoning_effort,
                    true,
                ));
            }
            Picker::Efforts {
                models,
                model,
                model_index,
                standard,
                advanced,
                index,
                return_to_models,
            } => {
                if let Some(option) = standard.get(index) {
                    self.apply_model_selection(&model.model, &option.effort)
                        .await;
                } else if !advanced.is_empty() {
                    let selected = advanced
                        .iter()
                        .position(|option| {
                            model.model == self.model && option.effort == self.reasoning_effort
                        })
                        .unwrap_or(0);
                    self.picker = Some(Picker::AdvancedEfforts {
                        models,
                        model,
                        model_index,
                        standard_index: index,
                        options: advanced,
                        index: selected,
                        return_to_models,
                    });
                }
            }
            Picker::AdvancedEfforts {
                model,
                options,
                index,
                ..
            } => {
                if let Some(option) = options.get(index) {
                    self.apply_model_selection(&model.model, &option.effort)
                        .await;
                }
            }
        }
    }

    pub(super) async fn apply_model_selection(&mut self, model: &str, effort: &str) {
        let Some(codex) = self.codex.as_mut() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        match codex.set_model_and_effort(model, effort).await {
            Ok(()) => {
                self.model = model.to_owned();
                self.reasoning_effort = effort.to_owned();
                let conversation = self.state.active_conversation().unwrap_or_default().to_owned();
                if let Err(error) = self.state.observe_settings(&conversation, model, effort) { self.record_error(error); return; }
                self.status = "idle".into();
                self.record_chat(
                    Speaker::Notice,
                    format!("Using {model} · {effort} reasoning"),
                );
            }
            Err(error) => self.record_error(error),
        }
    }

    pub(super) fn settle_delegation_failure(&mut self) {
        match self.state.phase() {
            NorthPhase::Delegating => {
                let _ = self.state.fail_delegation_before_child();
            }
            NorthPhase::Settling => {
                let child_id = self.state.active_delegated_child().map(str::to_owned);
                if let Some(child_id) = child_id {
                    let _ = self.state.fail_delegation_after_child(&child_id);
                }
            }
            _ => {}
        }
    }

    pub(super) fn record_error(&mut self, error: NorthError) {
        self.status = "failed".into();
        let message = error.user_message();
        match self.state.append_chat("error", &message) {
            Ok(()) => self.project_chat(),
            Err(state_error) => {
                // A failed application cannot record its own diagnostic.
                self.append_transcript_error(format!("{message}\n{}", state_error.user_message()));
            }
        }
    }

    pub(super) async fn shutdown(&mut self) {
        if let Some(pending) = self.reconciliation.take() { let _ = pending.codex.shutdown().await; }
        if let Some(task) = self.reconnect_task.take() {
            task.abort();
            if let Ok(Ok(result)) = task.await { let _ = result.codex.shutdown().await; }
        }
        if let Some(task) = self.usage_task.take() {
            task.abort();
            let _ = task.await;
        }
        for task in self.request_errors.drain(..) {
            task.abort();
            let _ = task.await;
        }
        for (_, task) in std::mem::take(&mut self.prompt_responses) {
            task.abort();
            let _ = task.await;
        }
        for (_, task) in std::mem::take(&mut self.steering) {
            task.abort();
            let _ = task.await;
        }
        for (_, turn) in std::mem::take(&mut self.turns) {
            turn.task.abort();
            let _ = turn.task.await;
        }
        if let Some(codex) = self.codex.take() {
            if let Err(error) = codex.shutdown().await {
                self.record_error(error);
            }
        }
    }
}

pub(super) fn session_branch(cwd: &Path) -> String {
    crate::platform::git::run(cwd, &["branch", "--show-current"])
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_default()
}

pub(super) fn effort_label_ref(effort: &str) -> &'static str {
    match effort {
        "none" => "None", "minimal" => "Minimal", "low" => "Low", "medium" => "Medium",
        "high" => "High", "xhigh" => "Extra high", "max" => "Max", "ultra" => "Ultra", _ => "Other",
    }
}

pub(super) fn navigate_view(state: &mut NorthState, key: &KeyCode, composer_empty: bool) -> NorthResult<bool> {
    match key {
        KeyCode::Tab if composer_empty => state.navigate_view(true)?,
        KeyCode::BackTab => state.navigate_view(false)?,
        KeyCode::Right if composer_empty => state.navigate_view(true)?,
        KeyCode::Left if composer_empty => state.navigate_view(false)?,
        KeyCode::Esc if state.active_view() != "chat" => state.show_chat()?,
        _ => return Ok(false),
    }
    Ok(true)
}
