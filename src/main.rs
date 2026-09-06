mod agent_catalog;
mod clause_state;
mod codex;
mod command_surface;
mod composer;
mod error;
mod rpc;
mod prompts;
mod references;

use std::env;
use std::collections::BTreeMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use clause_state::{AttachmentIdentity, NorthPhase, NorthState};
use codex::{Codex, ConversationEntry, ConversationSnapshot};
use command_surface::{
    Picker, matching_commands, menu_direction, render_picker,
    render_reference_menu, render_slash_menu,
};
use composer::{Composer, ImageHandles, Submission};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use error::{NorthError, NorthResult};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use ratatui::{Frame, layout::Rect};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

type NorthTerminal = Terminal<CrosstermBackend<Stdout>>;

#[derive(Debug, Eq, PartialEq)]
enum NorthCommand {
    Tui,
    Help,
    Agents(Vec<String>),
}

const CLI_HELP: &str = "North — interactive coding workspace

Usage: north [COMMAND]

Run north with no arguments to open the TUI.

Commands:
  config agents [sync|status|on|off|path|inspect]  Manage skills and hooks
  help                                          Show this help

Options:
  -h, --help  Show this help";

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<NorthCommand> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(NorthCommand::Tui);
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "-h" | "--help" | "help") {
        return Ok(NorthCommand::Help);
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(NorthCommand::Agents(arguments[2..].to_vec()));
    }
    let kind = if arguments[0].starts_with('-') {
        "option"
    } else {
        "command"
    };
    Err(NorthError::Usage(format!(
        "unrecognized {kind} '{}'\n\nUsage: north [COMMAND]\n\nRun 'north' to open the TUI, or 'north --help' for help.",
        arguments.join(" ")
    )))
}

struct TerminalSession;

impl TerminalSession {
    fn enter() -> NorthResult<(Self, NorthTerminal)> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok((Self, terminal))
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

struct App {
    cwd: PathBuf,
    branch: String,
    state: NorthState,
    codex: Option<Codex>,
    connection: Option<rpc::Rpc>,
    events: Option<rpc::Events>,
    model: String,
    reasoning_effort: String,
    composer: Composer,
    transcript: Vec<(Speaker, String)>,
    status: String,
    turns: BTreeMap<String, RunningTurn>,
    editors: BTreeMap<String, Composer>,
    pending_images: BTreeMap<u64, ImageHandles>,
    steering: BTreeMap<u64, JoinHandle<NorthResult<()>>>,
    prompt_responses: BTreeMap<u64, JoinHandle<NorthResult<()>>>,
    request_errors: Vec<JoinHandle<NorthResult<()>>>,
    prompt_editor: tui_textarea::TextArea<'static>,
    prompt_editor_key: Option<(u64, u64)>,
    prompt_editors: BTreeMap<(u64, u64), tui_textarea::TextArea<'static>>,
    prompt_scroll: u16,
    transcript_search: Option<tui_textarea::TextArea<'static>>,
    picker: Option<Picker>,
    command_index: usize,
    reference_candidates: Option<Vec<references::Reference>>,
    reference_observation: Option<String>,
}

struct RunningTurn {
    task: JoinHandle<TurnCompletion>,
    input: Option<u64>,
    interrupt: Option<oneshot::Sender<()>>,
    started: Instant,
}

struct TurnCompletion {
    result: TurnResult,
}

enum TurnResult {
    Direct(NorthResult<codex::TurnOutcome>),
    Delegation {
        child_id: Option<String>,
        result: NorthResult<codex::DelegationOutcome>,
    },
}

#[derive(Clone, Copy)]
enum Speaker {
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
    fn open(cwd: PathBuf) -> NorthResult<Self> {
        let state = NorthState::open()?;
        let branch = session_branch(&cwd);
        Ok(Self {
            cwd,
            branch,
            state,
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
            steering: BTreeMap::new(),
            prompt_responses: BTreeMap::new(),
            request_errors: Vec::new(),
            prompt_editor: tui_textarea::TextArea::default(),
            prompt_editor_key: None,
            prompt_editors: BTreeMap::new(),
            prompt_scroll: 0,
            transcript_search: None,
            picker: None,
            command_index: 0,
            reference_candidates: None,
            reference_observation: None,
        })
    }

    async fn accept_submission(&mut self, mut submission: Submission) -> bool {
        let input = submission.text.clone();
        let previous_notice = self.state.notice().to_owned();
        let transition = self.state.accept_input(&input);
        if let Err(error) = transition {
            self.detach_images(submission.attachment_identities());
            self.record_error(error);
            return false;
        }
        if !self.state.input_is_command()
            && self.state.host_effect().is_some_and(|effect| effect.action() == "submit")
        {
            self.record_chat(Speaker::Operator, input);
        }
        let notice = self.state.notice();
        if notice != previous_notice && !notice.is_empty() {
            self.record_chat(Speaker::Notice, notice.to_owned());
        }

        let Some(effect) = self.state.host_effect() else {
            self.detach_images(submission.attachment_identities());
            return false;
        };
        let action = effect.action().to_owned();
        let payload = effect.payload().to_owned();
        if action != "quit"
            && let Err(error) = self.state.clear_host_effect()
        {
            self.detach_images(submission.attachment_identities());
            self.record_error(error);
            return false;
        }
        match action.as_str() {
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
            "resume-conversation" | "select-agent" => {
                self.detach_images(submission.attachment_identities());
                self.open_conversation_picker(&payload).await;
                false
            }
            "select-model" => {
                self.detach_images(submission.attachment_identities());
                self.open_model_picker();
                false
            }
            "select-effort" => {
                self.detach_images(submission.attachment_identities());
                self.open_effort_picker();
                false
            }
            "open-switchboard" => {
                self.detach_images(submission.attachment_identities());
                self.open_switchboard();
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
            "delegate" => {
                submission.text = payload;
                self.submit_delegation(submission);
                false
            }
            unknown => {
                self.detach_images(submission.attachment_identities());
                self.record_error(NorthError::Protocol(format!(
                    "Clause projected unknown host effect {unknown}"
                )));
                false
            }
        }
    }

    fn submit_direct(&mut self, submission: Submission) {
        let attachments = match self.state.submit() {
            Ok(attachments) => attachments,
            Err(error) => {
                self.record_error(error);
                return;
            }
        };
        let image_paths = match submission.image_paths(&attachments) {
            Ok(paths) => paths,
            Err(error) => {
                self.settle_direct_failure();
                self.record_error(NorthError::Protocol(error));
                return;
            }
        };
        let conversation = self.state.active_conversation().unwrap_or_default().to_owned();
        self.launch_direct(&conversation, submission.text.clone(), image_paths, Some(submission), None);
    }

    fn turn_session(&self) -> NorthResult<codex::TurnSession> {
        let conversation = self.state.active_conversation()
            .ok_or_else(|| NorthError::Protocol("No conversation selected".into()))?;
        self.turn_session_in(conversation)
    }

    fn turn_session_in(&self, conversation: &str) -> NorthResult<codex::TurnSession> {
        let connection = self.connection.clone()
            .ok_or_else(|| NorthError::Protocol("Codex connection is unavailable".into()))?;
        let context = self.state.conversation(conversation)
            .ok_or_else(|| NorthError::State("No conversation to run".into()))?;
        Ok(codex::TurnSession::new(connection, conversation.into(), context.model.clone()))
    }

    fn launch_direct(&mut self, conversation: &str, text: String, image_paths: Vec<PathBuf>, keepalive: Option<Submission>, input: Option<u64>) {
        let mut session = match self.turn_session_in(conversation) {
            Ok(session) => session,
            Err(error) => {
                if let Some(number) = input {
                    self.record_input_receipt(number, "not sent");
                }
                let _ = self.state.settle_direct_in(conversation, false);
                self.record_error_in(conversation, error);
                return;
            }
        };
        self.status = "working".into();
        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = session
                .run_turn_interruptible(&text, &image_paths, interrupt_rx)
                .await;
            drop(keepalive);
            TurnCompletion {
                result: TurnResult::Direct(result),
            }
        });
        self.turns.insert(conversation.into(), RunningTurn {
            task, input, interrupt: Some(interrupt_tx), started: Instant::now(),
        });
    }

    fn retain_input(&mut self, submission: Submission, queued: bool) {
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

    async fn handle_composer_key(&mut self, key: KeyEvent) -> bool {
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
                self.command_index = 0;
                let removed = self.composer.handle_key(key);
                self.detach_images(removed);
            }
        }
        false
    }

    fn pending_image_paths(&self, number: u64) -> NorthResult<Vec<PathBuf>> {
        let input = self.state.pending_inputs().iter().find(|input| input.number == number)
            .ok_or_else(|| NorthError::Protocol("Pending input is missing".into()))?;
        self.pending_images.get(&number)
            .ok_or_else(|| NorthError::Protocol("Pending input image handles are missing".into()))?
            .image_paths(&input.attachments).map_err(NorthError::Protocol)
    }

    fn record_input_receipt(&mut self, number: u64, receipt: &str) {
        if let Err(error) = self.state.input_receipt(number, receipt) {
            self.record_error(error);
        } else {
            self.project_chat();
        }
    }

    fn forget_input(&mut self, number: u64) {
        match self.state.forget_input(number) {
            Ok(()) => { self.pending_images.remove(&number); }
            Err(error) => self.record_error(error),
        }
    }

    fn edit_pending_input(&mut self) -> NorthResult<()> {
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

    fn dispatch_pending_input(&mut self) -> NorthResult<()> {
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
            self.steering.insert(number, tokio::spawn(async move {
                codex::steer_turn(&connection, &thread, &turn, &text, &images).await
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
            self.launch_direct(&conversation, text, images, None, Some(number));
        }
        Ok(())
    }

    async fn collect_steering(&mut self) {
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

    fn submit_delegation(&mut self, submission: Submission) {
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

    fn detach_images(&mut self, identities: Vec<AttachmentIdentity>) {
        for identity in identities {
            if let Err(error) = self.state.detach_image(identity) {
                self.record_error(error);
            }
        }
    }

    fn is_working(&self) -> bool {
        self.turns.contains_key(self.state.active_conversation().unwrap_or_default())
    }

    fn record_chat(&mut self, speaker: Speaker, text: String) {
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

    fn project_chat(&mut self) {
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
    }

    fn collect_events(&mut self) {
        loop {
            let Some(events) = self.events.as_mut() else { break; };
            let message = match events.try_recv() {
                Ok(Ok(message)) => message,
                Ok(Err(error)) => {
                    self.events = None;
                    self.record_error(NorthError::Protocol(error));
                    break;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => {
                    self.events = None;
                    self.record_error(NorthError::Protocol(format!("Event stream needs reconciliation: {error}")));
                    break;
                }
            };
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
                        if method == "turn/started"
                            && let Some(number) = self.turns.get(conversation).and_then(|turn| turn.input)
                        {
                            self.record_input_receipt(number, "accepted");
                        }
                    }
                }
                _ => {}
            }
            for item in codex::chat_updates(&message) {
                let observation = clause_state::ChatEntryInput {
                    conversation: &item.conversation, turn: &item.turn, key: &item.key,
                    kind: &item.kind, text: &item.text, status: &item.status, append: item.append,
                };
                if let Err(error) = self.state.observe_chat_item(&observation) {
                    self.record_error(error);
                    return;
                }
                self.project_chat();
            }
        }
    }

    fn sync_prompt_editor(&mut self) {
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
            prompt.number == *number && prompt.status == "waiting"
                && prompt.questions.iter().any(|item| item.number == *question && !item.answered)));
    }

    fn reject_server_request(&mut self, request: &serde_json::Value, code: i64, description: &str) {
        let Some(connection) = self.connection.clone() else { return; };
        let response = serde_json::json!({"id":request["id"],"error":{"code":code,"message":description}});
        self.request_errors.push(tokio::spawn(async move { connection.send(response).await }));
        self.record_chat(Speaker::Notice, description.into());
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> NorthResult<bool> {
        if self.picker.is_some() { return Ok(false); }
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

    fn dispatch_prompt_response(&mut self) -> NorthResult<()> {
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

    async fn collect_prompt_responses(&mut self) {
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

    async fn collect_finished_turn(&mut self) {
        self.collect_events();
        let finished = self.turns.iter().filter_map(|(id, turn)| turn.task.is_finished().then_some(id.clone())).collect::<Vec<_>>();
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
                TurnResult::Direct(Err(NorthError::Interrupted)) => (
                    self.state.settle_interrupted_in(&conversation, None), Some(NorthError::Interrupted),
                ),
                TurnResult::Direct(result) => (self.state.settle_direct_in(&conversation, result.is_ok()), result.err()),
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

    fn interrupt_turn(&mut self) {
        let conversation = self.state.active_conversation().unwrap_or_default();
        if let Some(interrupt) = self.turns.get_mut(conversation).and_then(|turn| turn.interrupt.take()) {
            let _ = interrupt.send(());
        }
    }

    fn record_chat_in(&mut self, conversation: &str, kind: &str, text: &str) {
        if let Err(error) = self.state.append_chat_in(conversation, kind, text) {
            self.transcript.push((Speaker::System, error.user_message()));
        } else { self.project_chat(); }
    }

    fn record_error_in(&mut self, conversation: &str, error: NorthError) {
        if self.state.active_conversation() == Some(conversation) { self.status = "failed".into(); }
        self.record_chat_in(conversation, "error", &error.user_message());
    }

    async fn ensure_codex(&mut self) -> NorthResult<()> {
        if self.codex.is_none() {
            let mut codex = Codex::connect(&self.cwd).await?;
            let connection = codex.connection();
            self.events = Some(connection.subscribe());
            self.connection = Some(connection);
            let conversations = codex.conversations(&self.cwd).await?;
            for conversation in &conversations {
                self.state.observe_conversation(&conversation.id)?;
            }
            if let Some(conversation) = conversations.first() {
                self.state.request_switch_conversation(&conversation.id)?;
                let snapshot = match codex.resume_conversation(&conversation.id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        self.state.fail_switch_conversation(&conversation.id)?;
                        return Err(error);
                    }
                };
                self.state.settle_switch_conversation(&conversation.id)?;
                self.load_conversation(snapshot);
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

    async fn new_conversation(&mut self) {
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

    async fn open_conversation_picker(&mut self, query: &str) {
        let Some(codex) = self.codex.as_mut() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        let conversations = if query.is_empty() {
            codex.conversations(&self.cwd).await
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
                self.picker = Picker::conversations(conversations);
                if self.picker.is_none() {
                    self.record_chat(
                        Speaker::Notice,
                        "No previous conversations in this directory".into(),
                    );
                }
            }
            Err(error) => self.record_error(error),
        }
    }

    async fn switch_conversation(&mut self, conversation_id: &str) {
        if self.state.active_conversation() == Some(conversation_id) {
            return;
        }
        let previous = self.state.active_conversation().unwrap_or_default().to_owned();
        if let Err(error) = self.state.save_draft(&self.composer.text()) { self.record_error(error); return; }
        if let Err(error) = self.state.request_switch_conversation(conversation_id) {
            self.record_error(error);
            return;
        }
        if let Some(context) = self.state.conversation(conversation_id).filter(|context| context.attached) {
            if let Some(codex) = self.codex.as_mut() {
                codex.select_attached_conversation(conversation_id, &context.model, &context.effort);
            }
            match self.state.settle_switch_conversation(conversation_id) {
                Ok(()) => self.focus_conversation(&previous),
                Err(error) => self.record_error(error),
            }
            return;
        }
        let Some(codex) = self.codex.as_mut() else {
            let _ = self.state.fail_switch_conversation(conversation_id);
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        match codex.resume_conversation(conversation_id).await {
            Ok(snapshot) => match self.state.settle_switch_conversation(conversation_id) {
                Ok(()) => {
                    self.focus_conversation(&previous);
                    self.load_conversation(snapshot);
                }
                Err(error) => self.record_error(error),
            },
            Err(error) => {
                let _ = self.state.fail_switch_conversation(conversation_id);
                self.record_error(error);
            }
        }
    }

    fn load_conversation(&mut self, snapshot: ConversationSnapshot) {
        if let Err(error) = self.state.observe_settings(&snapshot.id, &snapshot.model, &snapshot.reasoning_effort) {
            self.record_error(error);
            return;
        }
        self.model = snapshot.model;
        self.reasoning_effort = snapshot.reasoning_effort;
        if let Err(error) = self.state.clear_chat() {
            self.record_error(error);
            return;
        }
        let entries = snapshot
            .entries
            .into_iter()
            .map(|entry| match entry {
                ConversationEntry::Operator(message) => (Speaker::Operator, message),
                ConversationEntry::Agent(message) => (Speaker::North, message),
                ConversationEntry::Command(command) => (
                    if command.succeeded {
                        Speaker::CommandSuccess
                    } else {
                        Speaker::CommandFailure
                    },
                    command.command,
                ),
            })
            .collect::<Vec<_>>();
        for (speaker, text) in entries {
            self.record_chat(speaker, text);
        }
        self.project_chat();
    }

    fn focus_conversation(&mut self, previous: &str) {
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
        self.command_index = 0;
        self.transcript_search = None;
        self.reference_observation = None;
        self.project_chat();
        self.sync_prompt_editor();
    }

    fn handle_transcript_key(&mut self, key: KeyEvent) -> NorthResult<bool> {
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

    fn displayed_messages(&self) -> String {
        self.transcript.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join("\n\n")
    }

    fn open_model_picker(&mut self) {
        let Some(codex) = self.codex.as_ref() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        self.picker = Picker::models(codex.models().to_vec(), &self.model);
        if self.picker.is_none() {
            self.record_error(NorthError::Protocol(
                "Codex returned no selectable models".into(),
            ));
        }
    }

    fn open_effort_picker(&mut self) {
        let Some(codex) = self.codex.as_ref() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        let models = codex.models().to_vec();
        let Some(index) = models.iter().position(|model| model.model == self.model) else {
            self.record_error(NorthError::Protocol(format!(
                "active model {} is absent from the Codex catalog",
                self.model
            )));
            return;
        };
        self.picker = Some(Picker::efforts(
            models,
            index,
            &self.model,
            &self.reasoning_effort,
            false,
        ));
    }

    fn open_switchboard(&mut self) {
        match agent_catalog::activation_units() {
            Ok(units) => self.picker = Some(Picker::switchboard(units)),
            Err(error) => self.record_error(error),
        }
    }

    fn toggle_switchboard_selection(&mut self) {
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

    fn reference_switchboard_selection(&mut self) {
        let Some(Picker::Switchboard { units, index }) = self.picker.as_ref() else {
            return;
        };
        let Some(unit) = units.get(*index) else {
            return;
        };
        self.composer
            .insert_reference(&unit.id, &unit.kind, &unit.source, false);
        self.picker = None;
        self.command_index = 0;
    }

    fn reference_query(&self) -> Option<String> {
        let query = self.composer.reference_query()?;
        (self.state.references_open() && self.state.reference_query() == query).then_some(query)
    }

    fn refresh_reference_menu(&mut self) {
        if self.picker.is_some() || self.state.active_prompt().is_some() || self.transcript_search.is_some() { return; }
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

    fn handle_reference_key(&mut self, key: &KeyEvent) -> bool {
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

    async fn accept_picker_selection(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        match picker {
            Picker::Conversations {
                conversations,
                index,
            } => {
                if let Some(conversation) = conversations.get(index) {
                    self.switch_conversation(&conversation.id).await;
                }
            }
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

    async fn apply_model_selection(&mut self, model: &str, effort: &str) {
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

    fn settle_direct_failure(&mut self) {
        if self.state.phase() == NorthPhase::Dispatching {
            let _ = self.state.settle_failure();
        }
    }

    fn settle_delegation_failure(&mut self) {
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

    fn record_error(&mut self, error: NorthError) {
        self.status = "failed".into();
        let message = error.user_message();
        match self.state.append_chat("error", &message) {
            Ok(()) => self.project_chat(),
            Err(state_error) => {
                // A failed application cannot record its own diagnostic.
                self.transcript.push((Speaker::System, format!("{message}\n{}", state_error.user_message())));
            }
        }
    }

    async fn shutdown(&mut self) {
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

fn session_branch(cwd: &Path) -> String {
    Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "not a Git worktree".into())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            if matches!(error, NorthError::Usage(_)) {
                ExitCode::from(2)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

async fn run_cli() -> NorthResult<()> {
    match parse_command(env::args().skip(1))? {
        NorthCommand::Help => {
            println!("{CLI_HELP}");
            return Ok(());
        }
        NorthCommand::Agents(arguments) => return agent_catalog::run(&arguments),
        NorthCommand::Tui => {}
    }
    let cwd = env::current_dir()?;
    let mut app = App::open(cwd)?;
    let (_session, mut terminal) = TerminalSession::enter()?;
    app.status = "connecting".into();
    draw(&mut terminal, &mut app)?;
    match app.ensure_codex().await {
        Ok(()) => app.status = "idle".into(),
        Err(error) => app.record_error(error),
    }
    let result = run(&mut terminal, &mut app).await;
    app.shutdown().await;
    terminal.show_cursor()?;
    result
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn config_agents_arguments_dispatch_before_terminal_entry() {
        assert_eq!(
            parse_command(["config", "agents", "sync"].map(str::to_owned)).unwrap(),
            NorthCommand::Agents(vec!["sync".into()])
        );
    }

    #[test]
    fn no_arguments_select_the_tui() {
        assert_eq!(
            parse_command(Vec::<String>::new()).unwrap(),
            NorthCommand::Tui
        );
    }

    #[test]
    fn unknown_arguments_do_not_fall_through_to_the_tui() {
        for arguments in [vec!["bridge"], vec!["config", "unknown"], vec!["--unknown"]] {
            let error = parse_command(arguments.into_iter().map(str::to_owned)).unwrap_err();
            assert!(matches!(error, NorthError::Usage(_)));
            assert!(error.to_string().contains("unrecognized"));
            assert!(error.to_string().contains("north --help"));
        }
    }

    #[test]
    fn help_dispatches_without_opening_the_terminal() {
        for option in ["help", "--help", "-h"] {
            assert_eq!(
                parse_command([option.to_owned()]).unwrap(),
                NorthCommand::Help
            );
        }
        assert!(CLI_HELP.contains("no arguments to open the TUI"));
    }
}

async fn run(terminal: &mut NorthTerminal, app: &mut App) -> NorthResult<()> {
    loop {
        app.collect_events();
        app.collect_prompt_responses().await;
        app.collect_steering().await;
        app.collect_finished_turn().await;
        if let Err(error) = app.dispatch_prompt_response() { app.record_error(error); }
        if let Err(error) = app.dispatch_pending_input() { app.record_error(error); }
        app.refresh_reference_menu();
        draw(terminal, app)?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let terminal_event = event::read()?;
        if let Event::Paste(pasted) = terminal_event {
            app.sync_prompt_editor();
            if let Some(editor) = app.transcript_search.as_mut() {
                editor.insert_str(pasted.replace('\r', "\n"));
            } else if app.state.active_prompt().is_some() {
                app.prompt_editor.insert_str(pasted.replace('\r', "\n"));
            } else {
                app.composer.insert_text(&pasted.replace('\r', "\n"));
            }
            continue;
        }
        let Event::Key(key) = terminal_event else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
            let mut command = Composer::new();
            command.insert_text("/agents");
            app.accept_submission(command.take_submission()).await;
            continue;
        }
        if app.handle_prompt_key(key)? { continue; }
        if app.picker.is_some() {
            if let Some(delta) = menu_direction(&key) {
                app.picker.as_mut().unwrap().move_selection(delta);
                continue;
            }
            match key.code {
                KeyCode::Esc => {
                    app.picker = app.picker.take().and_then(Picker::back);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.picker.as_mut().unwrap().move_selection(-1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.picker.as_mut().unwrap().move_selection(1);
                }
                KeyCode::Char(' ') if matches!(app.picker, Some(Picker::Switchboard { .. })) => {
                    app.toggle_switchboard_selection();
                }
                KeyCode::Char('@') if matches!(app.picker, Some(Picker::Switchboard { .. })) => {
                    app.reference_switchboard_selection();
                }
                KeyCode::Enter if matches!(app.picker, Some(Picker::Switchboard { .. })) => {
                    if let Some(Picker::Switchboard { units, index }) = app.picker.as_ref()
                        && let Some(unit) = units.get(*index)
                        && let Err(error) = edit_source(terminal, &unit.source)
                    {
                        app.record_error(error);
                    }
                }
                KeyCode::Enter => app.accept_picker_selection().await,
                _ => {}
            }
            continue;
        }
        if app.handle_reference_key(&key) {
            continue;
        }
        if app.handle_transcript_key(key)? { continue; }
        if key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::ALT) {
            if let Err(error) = app.edit_pending_input() { app.record_error(error); }
            continue;
        }
        if key.code == KeyCode::Esc && app.is_working() {
            app.interrupt_turn();
            continue;
        }
        if matches!(key.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'v'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            match Composer::read_clipboard_image() {
                Ok(file) => match app.state.attach_image() {
                    Ok(identity) => app.composer.attach_image(identity, file),
                    Err(error) => app.record_error(error),
                },
                Err(error) => {
                    app.record_chat(
                        Speaker::System,
                        format!("Could not paste clipboard image: {error}"),
                    );
                }
            }
            continue;
        }
        if navigate_view(&mut app.state, &key.code, app.composer.is_empty())? {
            continue;
        }
        let commands = matching_commands(app.state.commands(), &app.composer.text());
        if !commands.is_empty() {
            app.command_index = app.command_index.min(commands.len() - 1);
            if let Some(delta) = menu_direction(&key) {
                app.command_index = (app.command_index as isize + delta)
                    .rem_euclid(commands.len() as isize)
                    as usize;
                continue;
            }
            match key.code {
                KeyCode::Up => {
                    app.command_index = app
                        .command_index
                        .checked_sub(1)
                        .unwrap_or(commands.len() - 1);
                    continue;
                }
                KeyCode::Down => {
                    app.command_index = (app.command_index + 1) % commands.len();
                    continue;
                }
                KeyCode::Tab => {
                    let command = commands[app.command_index];
                    let removed = app.composer.replace_text(command.name());
                    app.detach_images(removed);
                    app.command_index = 0;
                    continue;
                }
                KeyCode::Enter => {
                    let command = commands[app.command_index];
                    let removed = app.composer.replace_text(command.name());
                    app.detach_images(removed);
                    let submission = app.composer.take_submission();
                    if app.accept_submission(submission).await {
                        break;
                    }
                    app.command_index = 0;
                    continue;
                }
                _ => {}
            }
        }
        if app.handle_composer_key(key).await { break; }
    }
    Ok(())
}

fn editor_command(editor: &str, source: &Path) -> Command {
    let mut command = Command::new("sh");
    // The editor is operator-configured shell syntax; the source stays a literal argument.
    command
        .arg("-c")
        .arg(format!("exec {editor} \"$1\""))
        .arg("north-editor")
        .arg(source);
    command
}

fn edit_source(terminal: &mut NorthTerminal, source: &Path) -> NorthResult<()> {
    let editor = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".into());
    disable_raw_mode()?;
    let result = (|| {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableBracketedPaste
        )?;
        editor_command(&editor, source).status()
    })();
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;
    // This is a new screen, not an in-place clear that must recover the editor's cursor.
    *terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let status = result?;
    if !status.success() {
        return Err(NorthError::Configuration(format!(
            "Editor exited with {status}"
        )));
    }
    Ok(())
}

fn navigate_view(state: &mut NorthState, key: &KeyCode, composer_empty: bool) -> NorthResult<bool> {
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

fn draw(terminal: &mut NorthTerminal, app: &mut App) -> NorthResult<()> {
    terminal.draw(|frame| render(frame, app))?;
    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &mut App) {
    let area = padded(frame.area());
    let editor_width = area.width.saturating_sub(2).max(1);
    let composer_height = app
        .composer
        .measure(editor_width)
        .min(area.height.saturating_sub(3).max(1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(composer_height),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    app.sync_prompt_editor();
    if let Some(picker) = app.picker.as_ref() {
        render_picker(frame, rows[0], picker, &app.model, &app.reasoning_effort);
    } else if let Some(prompt) = app.state.active_prompt() {
        prompts::render(frame, rows[0], prompt, &app.prompt_editor.lines().join("\n"), app.prompt_scroll);
    } else {
        match app.state.active_view() {
            "chat" => {
                let context = app.state.conversation(app.state.active_conversation().unwrap_or_default());
                let filtered = context.is_some_and(|context| context.transcript_changes || !context.transcript_query.is_empty());
                if app.transcript.is_empty() && app.state.pending_inputs().is_empty() && !filtered {
                    render_welcome(frame, rows[0], app);
                } else if app.transcript.is_empty() && filtered {
                    frame.render_widget(Paragraph::new("No matching messages. Ctrl+F edits the search; Ctrl+D toggles changes."), rows[0]);
                } else {
                    let transcript_area = if filtered { Rect {y: rows[0].y.saturating_add(1), height: rows[0].height.saturating_sub(1), ..rows[0]} } else { rows[0] };
                    let width = usize::from(transcript_area.width.max(1));
                    let transcript = conversation_text(app, width);
                    let line_count = transcript
                        .lines
                        .iter()
                        .map(|line| line.width().max(1).div_ceil(width))
                        .sum::<usize>();
                    let limit = line_count
                        .saturating_sub(transcript_area.height as usize)
                        .min(u16::MAX as usize) as u16;
                    let id = app.state.active_conversation().unwrap_or_default().to_owned();
                    let offset = app.state.conversation(&id).map(|context| context.transcript_offset).unwrap_or_default();
                    let hidden_lines = limit.saturating_sub(offset.min(u16::MAX as u64) as u16);
                    frame.render_widget(
                        Paragraph::new(transcript)
                            .wrap(Wrap { trim: false })
                            .scroll((hidden_lines, 0)),
                        transcript_area,
                    );
                    if app.state.conversation(&id).is_some_and(|context| context.transcript_limit != u64::from(limit)) {
                        if let Err(error) = app.state.size_transcript(u64::from(limit)) { app.record_error(error); }
                    }
                    if let Some(context) = app.state.conversation(&id) {
                        let mut heading = Vec::new();
                        if context.transcript_changes { heading.push("Changes only · Ctrl+D shows all".to_owned()); }
                        if !context.transcript_query.is_empty() { heading.push(format!("Search: {} · Ctrl+F edits", context.transcript_query)); }
                        if !heading.is_empty() { frame.render_widget(Paragraph::new(heading.join(" · ")).style(Style::default().fg(Color::Yellow)), Rect {height:1, ..rows[0]}); }
                    }
                }
            }
            "goals" => frame.render_widget(
                Paragraph::new(goals_text(&app.state)).wrap(Wrap { trim: false }),
                rows[0],
            ),
            unknown => frame.render_widget(
                Paragraph::new(format!("Unknown projected view: {unknown}")),
                rows[0],
            ),
        }
    }

    let composer_style = Style::default()
        .fg(Color::Rgb(229, 231, 235))
        .bg(Color::Rgb(37, 39, 45));
    frame.render_widget(Paragraph::new("").style(composer_style), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled("❯ ", composer_style.fg(Color::Cyan))),
        Rect {
            width: rows[1].width.min(2),
            ..rows[1]
        },
    );
    if rows[1].width > 2 {
        frame.render_widget(
            app.composer.textarea(),
            Rect {
                x: rows[1].x.saturating_add(2),
                width: rows[1].width.saturating_sub(2),
                ..rows[1]
            },
        );
    }
    if app.picker.is_none() && app.state.active_prompt().is_none() {
        if app.reference_query().is_some() {
            render_reference_menu(
                frame,
                rows[1],
                app.state.references(),
                app.state.reference_selection(),
            );
        } else {
            render_slash_menu(
                frame,
                rows[1],
                app.state.commands(),
                &app.composer.text(),
                app.command_index,
            );
        }
    }

    let active_tab_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive_tab_style = Style::default().fg(Color::DarkGray);
    let tab_style = |view| {
        if app.state.active_view() == view {
            active_tab_style
        } else {
            inactive_tab_style
        }
    };
    let view_context = match app.state.active_view() {
        "chat" => format!(
            "{} {} · {} · {}",
            app.model,
            app.reasoning_effort,
            app.cwd.display(),
            app.branch
        ),
        "goals" => "desired outcomes".into(),
        other => other.into(),
    };
    let mut tabs = Vec::new();
    for (index, view) in app.state.views().iter().enumerate() {
        if index > 0 {
            tabs.push(Span::styled(" | ", inactive_tab_style));
        }
        tabs.push(Span::styled(view.label.as_str(), tab_style(view.name.as_str())));
    }
    tabs.push(Span::styled(" > ", inactive_tab_style));
    tabs.push(Span::raw(view_context));
    frame.render_widget(Paragraph::new(Line::from(tabs)), rows[2]);

    let mut footer = vec![
        Span::styled("› ", Style::default().fg(Color::Cyan)),
        Span::styled(app.state.active_conversation().map(|id| format!("Conversation {}", id.chars().take(8).collect::<String>()))
            .unwrap_or_else(|| "Main".into()), Style::default().add_modifier(Modifier::BOLD)),
    ];
    if app.turns.len() > 1 {
        footer.push(Span::raw(format!(" · {} working", app.turns.len())));
    }
    if app.status == "failed" {
        footer.extend([
            Span::raw(" · "),
            Span::styled("failed", Style::default().fg(Color::Red)),
        ]);
    }
    footer.push(Span::styled(
        if app.is_working() { " · Ctrl+G agents · Enter steer · Tab queue · Esc interrupt" }
        else { " · / commands · Ctrl+G agents · Tab queue · Alt+↑ edit queued" },
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(footer)), rows[3]);
    if let Some(editor) = app.transcript_search.as_ref() {
        frame.render_widget(Paragraph::new(format!("Find: {} · Enter filter · Esc cancel", editor.lines().join(" ")))
            .style(Style::default().fg(Color::Yellow)), rows[3]);
    } else if app.status == "Copied displayed messages" {
        frame.render_widget(Paragraph::new("Copied displayed messages"), rows[3]);
    }
}

fn render_welcome(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }
    let card = Rect {
        x: area.x,
        y: area.y,
        width: area.width.min(72),
        height: area.height.min(7),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(card);
    frame.render_widget(block, card);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(">_ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("North (v{})", env!("CARGO_PKG_VERSION")),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::default(),
            welcome_field("model:", &format!("{} {}", app.model, app.reasoning_effort)),
            welcome_field("directory:", &app.cwd.display().to_string()),
            welcome_field("permissions:", "workspace write; approvals follow your settings"),
        ]),
        inner,
    );
}

fn welcome_field<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), Style::default().fg(Color::DarkGray)),
        Span::raw(value),
    ])
}

fn goals_text(state: &NorthState) -> Text<'static> {
    if state.goals().is_empty() {
        return Text::from(vec![
            Line::from("No Goals"),
            Line::from(""),
            Line::from(Span::styled(
                "Use /goal to create one.",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
    }
    let active_order = state.active_goal().map(|goal| goal.order());
    let mut lines = Vec::new();
    for (index, goal) in state.goals().iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        let active = active_order == Some(goal.order());
        lines.push(Line::from(vec![
            Span::styled(
                if active { "● " } else { "○ " },
                Style::default().fg(if active {
                    Color::Green
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled(
                goal.title().to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  #{} · {}", goal.order(), goal.status()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(format!("  {}", goal.objective())));
        if !goal.prior_objectives().is_empty() {
            lines.push(Line::from(Span::styled(
                "  Previous objectives",
                Style::default().fg(Color::DarkGray),
            )));
            lines.extend(goal.prior_objectives().iter().map(|objective| {
                Line::from(Span::styled(
                    format!("    · {objective}"),
                    Style::default().fg(Color::DarkGray),
                ))
            }));
        }
    }
    Text::from(lines)
}

fn conversation_text(app: &App, width: usize) -> Text<'_> {
    let mut lines = Vec::new();
    for (index, (speaker, message)) in app.transcript.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        match speaker {
            Speaker::Operator => {
                let style = Style::default()
                    .fg(Color::Rgb(226, 220, 199))
                    .bg(Color::Rgb(52, 58, 62));
                for (line_index, line) in wrap_operator_message(message, width).iter().enumerate() {
                    let marker = if line_index == 0 { "› " } else { "  " };
                    let content = format!(
                        "{marker}{line:<padding$}",
                        padding = width.saturating_sub(2)
                    );
                    lines.push(Line::from(content).style(style));
                }
            }
            Speaker::North => {
                let options = tui_markdown::Options::default()
                    .image_fallback(tui_markdown::ImageFallback::AltTextAndUrl);
                let mut markdown = tui_markdown::from_str_with_options(message, &options);
                if let Some(first_content) = markdown
                    .lines
                    .iter()
                    .position(|line| !line.spans.iter().all(|span| span.content.trim().is_empty()))
                {
                    for (line_index, line) in markdown.lines.iter_mut().enumerate() {
                        let prefix = if line_index == first_content {
                            Span::styled("• ", Style::default().fg(Color::Gray))
                        } else if line_index > first_content {
                            Span::raw("  ")
                        } else {
                            continue;
                        };
                        line.spans.insert(0, prefix);
                    }
                }
                lines.extend(markdown.lines);
            }
            Speaker::CommandSuccess | Speaker::CommandFailure | Speaker::CommandRunning => {
                let color = if matches!(speaker, Speaker::CommandSuccess) {
                    Color::Green
                } else if matches!(speaker, Speaker::CommandRunning) {
                    Color::Yellow
                } else {
                    Color::Red
                };
                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(color)),
                    Span::styled(if matches!(speaker, Speaker::CommandRunning) { "Running " } else { "Ran " }, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(message.lines().next().unwrap_or_default()),
                ]));
                lines.extend(message.lines().skip(1).map(|line| Line::from(format!("  {line}"))));
            }
            Speaker::FileChange => {
                lines.extend(message.lines().map(|line| Line::from(line).style(Style::default().fg(
                    if line.starts_with('+') { Color::Green } else if line.starts_with('-') { Color::Red } else { Color::Gray }
                ))));
            }
            Speaker::Notice => lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Gray)),
                Span::styled(message, Style::default().add_modifier(Modifier::DIM)),
            ])),
            Speaker::System => lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Red)),
                Span::styled(message, Style::default().fg(Color::Red)),
            ])),
        }
    }
    if app.is_working() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(working_line(app));
    }
    let conversation = app.state.active_conversation().unwrap_or_default();
    for input in app.state.pending_inputs().iter().filter(|input| input.conversation == conversation && input.status != "accepted") {
        lines.push(Line::from(format!("{} #{}: {}", input.status, input.number, input.text))
            .style(Style::default().fg(Color::Yellow)));
    }
    Text::from(lines)
}

fn wrap_operator_message(message: &str, width: usize) -> Vec<String> {
    let content_width = width.saturating_sub(2).max(1);
    let mut wrapped = Vec::new();
    for source_line in message.lines() {
        if source_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in source_line.split_whitespace() {
            let separator = usize::from(!line.is_empty());
            if line.chars().count() + separator + word.chars().count() <= content_width {
                if separator == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                wrapped.push(std::mem::take(&mut line));
            }
            let mut remaining = word;
            while remaining.chars().count() > content_width {
                let split = remaining
                    .char_indices()
                    .nth(content_width)
                    .map_or(remaining.len(), |(index, _)| index);
                wrapped.push(remaining[..split].to_owned());
                remaining = &remaining[split..];
            }
            line.push_str(remaining);
        }
        wrapped.push(line);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

fn working_line(app: &App) -> Line<'static> {
    let elapsed = app
        .turns.get(app.state.active_conversation().unwrap_or_default())
        .map(|turn| turn.started.elapsed())
        .unwrap_or_default();
    let mut spans = shimmer_spans("•", elapsed);
    spans.push(Span::raw(" "));
    spans.extend(shimmer_spans("Working", elapsed));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!(
            "({} • esc to interrupt)",
            fmt_elapsed_compact(elapsed.as_secs())
        ),
        Style::default().add_modifier(Modifier::DIM),
    ));
    Line::from(spans)
}

fn fmt_elapsed_compact(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        return format!("{}m {:02}s", elapsed_secs / 60, elapsed_secs % 60);
    }
    format!(
        "{}h {:02}m {:02}s",
        elapsed_secs / 3600,
        (elapsed_secs % 3600) / 60,
        elapsed_secs % 60
    )
}

fn shimmer_spans(text: &str, elapsed: Duration) -> Vec<Span<'static>> {
    let characters = text.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return Vec::new();
    }
    let padding = 10;
    let period = characters.len() + padding * 2;
    let position = ((elapsed.as_secs_f32() % 2.0) / 2.0 * period as f32) as isize;
    characters
        .into_iter()
        .enumerate()
        .map(|(index, character)| {
            let distance = (index as isize + padding as isize - position).unsigned_abs() as f32;
            let intensity = if distance <= 5.0 {
                let x = std::f32::consts::PI * (distance / 5.0);
                0.5 * (1.0 + x.cos())
            } else {
                0.0
            };
            let style = if intensity < 0.2 {
                Style::default().add_modifier(Modifier::DIM)
            } else if intensity < 0.6 {
                Style::default()
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            Span::styled(character.to_string(), style)
        })
        .collect()
}

fn padded(area: Rect) -> Rect {
    if area.width > 2 && area.height > 2 {
        area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        })
    } else {
        area
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;

    #[test]
    fn transcript_controls_scroll_filter_changes_and_copy_without_changing_the_draft() {
        let mut app = App::open(PathBuf::from("/tmp/north-transcript-test")).unwrap();
        app.composer.insert_text("keep my draft");
        for number in 0..20 { app.record_chat(Speaker::Operator, format!("message {number}")); }
        let screen = render_text(&mut app, 110, 12);
        assert!(screen.contains("message 19"), "{screen}");
        app.handle_transcript_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL)).unwrap();
        let screen = render_text(&mut app, 110, 12);
        assert!(screen.contains("message 0"), "{screen}");
        assert!(!screen.contains("message 19"), "{screen}");
        app.handle_transcript_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL)).unwrap();
        app.record_chat(Speaker::FileChange, "src/example.rs\n-old\n+ÅNGSTRÖM".into());
        app.record_chat(Speaker::North, "Ångström discussed in prose".into());
        app.handle_transcript_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)).unwrap();
        for character in "ångström".chars() {
            app.handle_transcript_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
        }
        app.handle_transcript_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
        assert!(app.displayed_messages().contains("discussed in prose"));
        assert!(!app.displayed_messages().contains("message 19"));
        app.handle_transcript_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.displayed_messages(), "src/example.rs\n-old\n+ÅNGSTRÖM");
        assert_eq!(app.composer.text(), "keep my draft");
    }

    fn hold_turn(app: &mut App) {
        let conversation = app.state.active_conversation().unwrap_or_default().to_owned();
        app.turns.insert(conversation, RunningTurn {
            task: tokio::spawn(std::future::pending()), input: None, interrupt: None, started: Instant::now(),
        });
    }

    #[tokio::test]
    async fn switching_running_conversations_keeps_drafts_and_interrupts_only_the_selected_turn() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
        use serde_json::{Value, json};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        for conversation in ["alpha", "beta"] {
            app.state.request_new_conversation().unwrap();
            app.state.settle_new_conversation(conversation).unwrap();
            app.state.observe_settings(conversation, "fixture-model", "high").unwrap();
        }
        app.switch_conversation("alpha").await;
        let (client, server) = duplex(16384);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.connection = Some(connection.clone());
        app.events = Some(events);
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        let mut image_paths = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), async {
            for conversation in ["alpha", "beta"] {
                app.switch_conversation(conversation).await;
                app.composer.insert_text(&format!("start {conversation}"));
                app.handle_composer_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await;
                let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(request["method"], "turn/start");
                assert_eq!(request["params"]["threadId"], conversation);
                let turn = format!("{conversation}-turn");
                for message in [
                    json!({"id":request["id"],"result":{"turn":{"id":turn}}}),
                    json!({"method":"turn/started","params":{"threadId":conversation,"turn":{"id":turn}}}),
                    json!({"method":"item/agentMessage/delta","params":{"threadId":conversation,"turnId":turn,"itemId":"progress","delta":format!("{conversation} is progressing")}}),
                ] { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
                while app.state.active_turn().is_empty() { app.collect_events(); tokio::task::yield_now().await; }
                app.composer.insert_text(&format!("draft {conversation}"));
                let image = app.state.attach_image().unwrap();
                let file = tempfile::NamedTempFile::new().unwrap();
                image_paths.push(file.path().to_owned());
                app.composer.attach_image(image, file);
            }
            assert_eq!(app.turns.len(), 2);
            app.switch_conversation("alpha").await;
            assert!(app.composer.text().contains("draft alpha"));
            assert!(!app.composer.text().contains("draft beta"));
            let screen = render_text(&mut app, 110, 25);
            assert!(screen.contains("alpha is progressing"), "{screen}");
            assert!(!screen.contains("beta is progressing"), "{screen}");
            assert!(image_paths.iter().all(|path| path.exists()));
            app.interrupt_turn();
            let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["method"], "turn/interrupt");
            assert_eq!(request["params"], json!({"threadId":"alpha","turnId":"alpha-turn"}));
            for message in [
                json!({"id":request["id"],"result":{}}),
                json!({"method":"turn/completed","params":{"threadId":"alpha","turn":{"id":"alpha-turn","status":"interrupted","items":[]}}}),
            ] { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
            while app.turns.contains_key("alpha") { app.collect_finished_turn().await; tokio::task::yield_now().await; }
            assert!(app.turns.contains_key("beta"));
            assert_eq!(app.state.conversation("beta").unwrap().phase, NorthPhase::Dispatching);
            assert!(!render_text(&mut app, 110, 25).contains("· failed"));
            app.switch_conversation("beta").await;
            assert!(app.composer.text().contains("draft beta"));
            assert!(app.is_working());
            let message = json!({"method":"turn/completed","params":{"threadId":"beta","turn":{"id":"beta-turn","status":"completed","items":[{"id":"answer","type":"agentMessage","text":"beta finished"}]}}});
            writer.write_all(format!("{message}\n").as_bytes()).await.unwrap();
            while !app.turns.is_empty() { app.collect_finished_turn().await; tokio::task::yield_now().await; }
            assert_eq!(app.state.phase(), NorthPhase::Completed);
            assert!(render_text(&mut app, 110, 25).contains("beta finished"));
            app.switch_conversation("alpha").await;
            assert_eq!(app.state.phase(), NorthPhase::Interrupted);
            assert!(!render_text(&mut app, 110, 25).contains("· failed"));
        }).await.unwrap();
        app.shutdown().await;
        connection.close().await;
        driver.await.unwrap();
        drop(app);
        assert!(image_paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn tool_forms_use_checked_choices_validate_fields_and_keep_the_chat_draft() {
        use serde_json::json;
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("thread").unwrap();
        app.composer.insert_text("unsent chat draft");
        let message = json!({"id":72,"method":"mcpServer/elicitation/request","params":{
            "threadId":"thread","serverName":"fixture-tool","mode":"form","message":"Configure this request",
            "requestedSchema":{"type":"object","required":["a_count","b_choices","d_empty","e_enabled","f_kind"],"properties":{
                "a_count":{"type":"integer","minimum":1,"maximum":3},
                "b_choices":{"type":"array","items":{"anyOf":[{"const":"red","title":"Red"},{"const":"blue","title":"Blue"}]},"minItems":1,"maxItems":1},
                "c_note":{"type":"string"},"d_empty":{"type":"string","maxLength":4},
                "e_enabled":{"type":"boolean"},"f_kind":{"type":"string","enum":["raw-small","raw-large"],"enumNames":["Small","Large"]}
            }}
        }});
        app.state.observe_prompt(&prompts::decode_request(&message, &[]).unwrap().unwrap()).unwrap();
        let key = |app: &mut App, code| { app.handle_prompt_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap(); };
        for character in "2.5".chars() { key(&mut app, KeyCode::Char(character)); }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.state.active_prompt().unwrap().current, 1);
        let screen = render_text(&mut app, 110, 28);
        assert!(screen.contains("Enter a whole number."), "{screen}");
        for _ in 0..2 { key(&mut app, KeyCode::Backspace); }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.state.active_prompt().unwrap().current, 2);
        key(&mut app, KeyCode::Enter);
        assert!(render_text(&mut app, 110, 28).contains("Provide at least 1 choices."));
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.state.active_prompt().unwrap().current, 2);
        assert!(render_text(&mut app, 110, 28).contains("Provide no more than 1 choices."));
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        app.handle_prompt_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert!(render_text(&mut app, 110, 28).contains("Send these answers?"));
        key(&mut app, KeyCode::Enter);
        app.state.prepare_prompt_response().unwrap().unwrap();
        assert_eq!(prompts::response(app.state.active_prompt().unwrap()).unwrap(), json!({"id":72,"result":{
            "action":"accept","content":{"a_count":2,"b_choices":["red"],"d_empty":"","e_enabled":true,"f_kind":"raw-large"}
        }}));
        assert_eq!(app.composer.text(), "unsent chat draft");
    }

    #[tokio::test]
    async fn unsupported_server_requests_receive_an_explicit_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
        use serde_json::{Value, json};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        let (client, server) = duplex(8192);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.connection = Some(connection.clone());
        app.events = Some(events);
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        writer.write_all(b"{\"id\":\"unknown-request\",\"method\":\"unsupported/request\",\"params\":{}}\n").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.request_errors.is_empty() { app.collect_events(); tokio::task::yield_now().await; }
        }).await.unwrap();
        let reply: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
        assert_eq!(reply["id"], json!("unknown-request"));
        assert_eq!(reply["error"]["code"], json!(-32601));
        assert!(reply.get("result").is_none());
        app.shutdown().await;
        connection.close().await;
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn server_questions_use_the_prompt_editor_preserve_the_draft_and_reply_once() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
        use serde_json::{Value, json};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("thread-prompt").unwrap();
        app.composer.insert_text("keep this draft");
        let image = app.state.attach_image().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        app.composer.attach_image(image, file);
        let original = app.composer.text();
        let (client, server) = duplex(8192);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.connection = Some(connection.clone());
        app.events = Some(events);
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        let question = json!({"id":"question-1","method":"item/tool/requestUserInput","params":{
            "threadId":"thread-prompt","turnId":"turn","itemId":"questions","questions":[
                {"id":"pick","header":"Approach","question":"How should we proceed?","options":[
                    {"label":"Small","description":"Minimal changes"},{"label":"Complete","description":"Finish the feature"}]},
                {"id":"private","header":"Private note","question":"Add a private note","isSecret":true,"options":null},
            ]
        }});
        writer.write_all(format!("{question}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.state.active_prompt().is_none() { app.collect_events(); tokio::task::yield_now().await; }
        }).await.unwrap();
        let screen = render_text(&mut app, 110, 25);
        assert!(screen.contains("How should we proceed?"), "{screen}");
        assert!(screen.contains("keep this draft"), "{screen}");
        app.handle_prompt_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).unwrap();
        app.handle_prompt_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
        for character in "masked fixture".chars() {
            app.handle_prompt_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
        }
        let screen = render_text(&mut app, 110, 25);
        assert!(screen.contains("Add a private note"), "{screen}");
        assert!(screen.contains("••••••"), "{screen}");
        assert!(!screen.contains("masked fixture"), "{screen}");
        app.handle_prompt_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
        app.dispatch_prompt_response().unwrap();
        let response: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
        assert_eq!(response, json!({"id":"question-1","result":{"answers":{
            "pick":{"answers":["Complete"]},"private":{"answers":["masked fixture"]},
        }}}));
        assert_eq!(app.composer.text(), original);
        assert!(path.exists());
        let resolved = json!({"method":"serverRequest/resolved","params":{"threadId":"thread-prompt","requestId":"question-1"}});
        writer.write_all(format!("{resolved}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.state.active_prompt().is_some() {
                app.collect_events(); app.collect_prompt_responses().await; tokio::task::yield_now().await;
            }
        }).await.unwrap();
        app.collect_prompt_responses().await;
        app.dispatch_prompt_response().unwrap();
        assert!(app.prompt_responses.is_empty());
        assert_eq!(app.composer.text(), original);
        assert!(app.prompt_editor.lines().join("\n").is_empty());
        assert_eq!(app.state.submit().unwrap(), vec![image]);
        assert!(path.exists());
        connection.close().await;
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn steering_keys_send_the_active_turn_and_keep_rejected_images_editable() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
        use serde_json::{Value, json};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("thread-steer").unwrap();
        app.state.submit().unwrap();
        hold_turn(&mut app);
        let (client, server) = duplex(8192);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.connection = Some(connection.clone());
        app.events = Some(events);
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        let started = json!({"method":"turn/started","params":{"threadId":"thread-steer","turn":{"id":"turn-steer"}}});
        writer.write_all(format!("{started}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.state.active_turn().is_empty() {
                app.collect_events(); tokio::task::yield_now().await;
            }
        }).await.unwrap();
        app.composer.insert_text("focus on this image");
        let attachment = app.state.attach_image().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        app.composer.attach_image(attachment, file);
        app.handle_composer_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await;
        let number = app.state.pending_inputs()[0].number;
        assert!(app.composer.is_empty());
        app.dispatch_pending_input().unwrap();
        let request: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "turn/steer");
        assert_eq!(request["params"]["expectedTurnId"], "turn-steer");
        assert_eq!(request["params"]["threadId"], "thread-steer");
        assert_eq!(request["params"]["input"][1], json!({"type":"localImage","path":path}));
        assert!(path.exists());
        app.composer.insert_text("keep my newer draft");
        let rejected = json!({"id":request["id"],"error":{"code":-32600,"message":"turn already ended"}});
        writer.write_all(format!("{rejected}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !app.steering.is_empty() { app.collect_steering().await; tokio::task::yield_now().await; }
        }).await.unwrap();
        assert_eq!(app.state.pending_inputs()[0].status, "not sent");
        assert!(path.exists());
        assert_eq!(app.composer.text(), "keep my newer draft");
        let screen = render_text(&mut app, 110, 24);
        assert!(screen.contains(&format!("not sent #{number}: focus on this image")), "{screen}");
        app.edit_pending_input().unwrap();
        assert_eq!(app.composer.text(), "keep my newer draft");
        app.composer.replace_text("");
        app.edit_pending_input().unwrap();
        assert!(app.state.pending_inputs().is_empty());
        assert!(app.composer.text().contains("focus on this image"));
        let restored = app.composer.take_submission();
        assert_eq!(restored.image_paths(&[attachment]).unwrap(), vec![path.clone()]);
        assert!(path.exists());
        drop(restored);
        assert!(!path.exists());
        app.shutdown().await;
        connection.close().await;
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn queued_key_dispatches_after_completion_without_consuming_the_newer_draft() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
        use serde_json::{Value, json};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("thread-queue").unwrap();
        app.state.submit().unwrap();
        app.composer.insert_text("queued image task");
        let attachment = app.state.attach_image().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_owned();
        app.composer.attach_image(attachment, file);
        assert!(!navigate_view(&mut app.state, &KeyCode::Tab, false).unwrap());
        app.handle_composer_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)).await;
        let number = app.state.pending_inputs()[0].number;
        app.dispatch_pending_input().unwrap();
        assert!(app.turns.is_empty());
        assert_eq!(app.state.pending_inputs()[0].status, "queued");
        app.composer.insert_text("my unsent draft");
        let newer_image = app.state.attach_image().unwrap();
        let newer_file = tempfile::NamedTempFile::new().unwrap();
        let newer_path = newer_file.path().to_owned();
        app.composer.attach_image(newer_image, newer_file);
        let draft = app.composer.text();
        app.state.settle_success().unwrap();
        let (client, server) = duplex(8192);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.connection = Some(connection.clone());
        app.events = Some(events);
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        app.dispatch_pending_input().unwrap();
        let request: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "turn/start");
        assert_eq!(request["params"]["input"].as_array().unwrap().len(), 2);
        assert_eq!(request["params"]["input"][1], json!({"type":"localImage","path":path}));
        assert!(path.exists());
        assert_eq!(app.composer.text(), draft);
        let messages = [
            json!({"id":request["id"],"result":{"turn":{"id":"turn-queued"}}}),
            json!({"method":"turn/started","params":{"threadId":"thread-queue","turn":{"id":"turn-queued"}}}),
            json!({"method":"turn/completed","params":{"threadId":"thread-queue","turn":{"id":"turn-queued","status":"completed","items":[{"type":"agentMessage","id":"reply","text":"queued result"}]}}}),
        ];
        for message in messages { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
        tokio::time::timeout(Duration::from_secs(5), async {
            while !app.turns.is_empty() {
                app.collect_events(); app.collect_finished_turn().await; tokio::task::yield_now().await;
            }
        }).await.unwrap();
        assert!(app.state.pending_inputs().is_empty());
        assert!(!app.pending_images.contains_key(&number));
        assert!(!path.exists());
        assert_eq!(app.composer.text(), draft);
        assert_eq!(app.state.submit().unwrap(), vec![newer_image]);
        assert!(newer_path.exists());
        let screen = render_text(&mut app, 110, 24);
        assert!(screen.contains("queued image task"), "{screen}");
        assert!(screen.contains("queued result"), "{screen}");
        app.shutdown().await;
        connection.close().await;
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn streamed_text_and_command_output_render_before_the_turn_finishes() {
        use tokio::io::{AsyncWriteExt, duplex, split};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::open(directory.path().to_owned()).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("thread-live").unwrap();
        app.record_chat(Speaker::Operator, "show live progress".into());
        app.state.submit().unwrap();
        hold_turn(&mut app);
        let (client, mut server) = duplex(8192);
        let (reader, writer) = split(client);
        let (connection, events, driver) = rpc::Rpc::start(reader, writer);
        app.events = Some(events);
        app.connection = Some(connection.clone());
        let notifications = [
            serde_json::json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-live","turnId":"turn-live","itemId":"message","delta":"Inspecting the files"}}),
            serde_json::json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"thread-live","turnId":"turn-live","itemId":"command","delta":"first output line\nsecond output line"}}),
        ];
        for message in notifications {
            server.write_all(format!("{message}\n").as_bytes()).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.events.as_ref().unwrap().len() < 2 { tokio::task::yield_now().await; }
        }).await.unwrap();
        app.collect_events();
        let screen = render_text(&mut app, 100, 24);
        assert!(screen.contains("Inspecting the files"), "{screen}");
        assert!(screen.contains("second output line"), "{screen}");
        assert!(app.is_working());
        let completed = serde_json::json!({"method":"item/completed","params":{"threadId":"thread-live","turnId":"turn-live","item":{"id":"message","type":"agentMessage","text":"Inspection complete"}}});
        server.write_all(format!("{completed}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while app.events.as_ref().unwrap().is_empty() { tokio::task::yield_now().await; }
        }).await.unwrap();
        app.collect_events();
        let screen = render_text(&mut app, 100, 24);
        assert_eq!(screen.matches("Inspection complete").count(), 1);
        assert!(!screen.contains("Inspecting the files"));
        app.shutdown().await;
        connection.close().await;
        driver.await.unwrap();
    }
    use ratatui::backend::TestBackend;

    fn render_text(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                rendered.push_str(buffer[(x, y)].symbol());
            }
            rendered.push('\n');
        }
        rendered
    }

    fn accepted_frame_app() -> App {
        let mut app = App::open(PathBuf::from("/tmp/demo")).unwrap();
        app.branch = "north-v2-usable-tui".into();
        app.model = "gpt-example".into();
        app.reasoning_effort = "high".into();
        app.composer.insert_text("next question");
        app.transcript = vec![
            (Speaker::Operator, "FIRST".into()),
            (Speaker::North, "first answer".into()),
            (Speaker::System, "visible diagnostic".into()),
        ];
        app.status = "complete".into();
        app
    }

    fn submission(text: &str) -> Submission {
        let mut composer = Composer::new();
        composer.insert_text(text);
        composer.take_submission()
    }

    #[tokio::test]
    async fn clause_drives_the_tui_goal_create_inspect_edit_inspect_journey() {
        let mut app = App::open(PathBuf::from("/tmp/demo")).unwrap();

        assert!(!app.accept_submission(submission("/goal")).await);
        assert_eq!(app.state.notice(), "Name the Goal");
        assert!(!app.accept_submission(submission("Build North")).await);
        assert_eq!(app.state.notice(), "Describe the desired outcome");
        assert!(
            !app.accept_submission(submission("Make Clause own Goals"))
                .await
        );
        assert_eq!(app.state.notice(), "Goal created");

        assert!(!app.accept_submission(submission("/goals")).await);
        let created = render_text(&mut app, 100, 18);
        assert!(created.contains("● Build North  #1 · active"));
        assert!(created.contains("Make Clause own Goals"));

        assert!(!app.accept_submission(submission("/goal edit")).await);
        assert_eq!(app.state.notice(), "Edit the desired outcome");
        assert_eq!(app.composer.text(), "Make Clause own Goals");
        assert!(
            !app.accept_submission(submission("Make Clause own the whole TUI"))
                .await
        );
        let edited = render_text(&mut app, 100, 20);
        assert!(edited.contains("Make Clause own the whole TUI"));
        assert!(edited.contains("Previous objectives"));
        assert!(edited.contains("Make Clause own Goals"));
    }

    #[tokio::test]
    async fn inline_goal_commands_edit_and_clear_without_submitting_a_turn() {
        let mut app = App::open(PathBuf::from("/tmp/demo")).unwrap();
        app.accept_submission(submission("/goal edit")).await;
        assert_eq!(app.state.notice(), "No selected goal");

        app.accept_submission(submission("  /goal\tship 世界  ")).await;
        assert_eq!(app.state.active_goal().unwrap().objective(), "ship 世界");
        app.accept_submission(submission("/goal edit ship the repaired harness")).await;
        let goal = app.state.active_goal().unwrap();
        assert_eq!(goal.objective(), "ship the repaired harness");
        assert_eq!(goal.prior_objectives(), ["ship 世界"]);

        app.accept_submission(submission("/goal clear extra")).await;
        assert_eq!(app.state.notice(), "Usage: /goal clear");
        assert!(app.state.active_goal().is_some());
        app.accept_submission(submission("/goal clear")).await;
        assert!(app.state.active_goal().is_none());
        assert_eq!(app.state.goals()[0].status(), "cleared");
        app.accept_submission(submission("/goal set second outcome")).await;
        assert_eq!(app.state.active_goal().unwrap().objective(), "second outcome");
        assert!(!app.is_working());
        assert!(app.state.host_effect().is_none());
    }

    #[test]
    fn source_resolves_unknown_commands_without_dispatching_chat() {
        let mut state = NorthState::open().unwrap();
        state.accept_input("/unknown argument").unwrap();
        assert_eq!(state.notice(), "Unknown command: /unknown");
        assert!(state.input_is_command());
        assert!(state.host_effect().is_none());
    }

    #[test]
    fn headless_surface_matches_the_accepted_product_frame() {
        let mut app = accepted_frame_app();
        let rendered = render_text(&mut app, 110, 12);

        assert!(
            rendered.contains(
                "Chat | Goals > gpt-example high · /tmp/demo · north-v2-usable-tui"
            )
        );
        assert!(rendered.contains("❯ next question"));
        assert!(rendered.contains("› FIRST"));
        assert!(rendered.contains("• first answer"));
        assert!(rendered.contains("• visible diagnostic"));
        assert!(rendered.contains("› Main · / commands"));
        assert!(!rendered.contains("you>"));
        assert!(!rendered.contains("north>"));
        assert!(!rendered.contains("Clause"));

        assert!(!rendered.contains("North TUI"));
        assert!(!rendered.contains("Conversation"));
        assert!(!rendered.contains("Prompt (/q to quit)"));
    }

    #[test]
    fn tab_and_arrow_keys_navigate_the_two_product_views() {
        let mut state = NorthState::open().expect("North Clause source opens");
        for (key, expected) in [
            (KeyCode::Tab, "goals"), (KeyCode::Right, "chat"),
            (KeyCode::Tab, "goals"), (KeyCode::Left, "chat"),
            (KeyCode::BackTab, "goals"), (KeyCode::Esc, "chat"),
        ] {
            assert!(navigate_view(&mut state, &key, true).unwrap());
            assert_eq!(state.active_view(), expected);
        }
        assert!(!navigate_view(&mut state, &KeyCode::Up, true).unwrap());
        assert!(!navigate_view(&mut state, &KeyCode::Left, false).unwrap());
    }

    #[test]
    fn goals_view_renders_its_empty_state() {
        let mut app = accepted_frame_app();
        app.state.execute_command("/goals").unwrap();
        let goals = render_text(&mut app, 110, 12);
        assert!(goals.contains("Chat | Goals > desired outcomes"));
        assert!(goals.contains("No Goals"));
        assert!(!goals.contains("first answer"));
    }

    #[test]
    fn slash_popup_exposes_the_basic_command_surface() {
        let mut app = accepted_frame_app();
        app.composer.replace_text("/");

        let rendered = render_text(&mut app, 100, 18);

        assert!(rendered.contains("/model"));
        assert!(rendered.contains("/effort"));
        assert!(rendered.contains("/new"));
        assert!(rendered.contains("/resume"));
        assert!(rendered.contains("/config"));
        assert!(rendered.contains("/delegate"));
        assert!(rendered.contains("choose the active model and reasoning effort"));
    }

    #[test]
    fn conversation_picker_and_replay_replace_the_prior_thread_completely() {
        let mut app = accepted_frame_app();
        app.picker = Picker::conversations(vec![codex::ConversationOption {
            id: "thread-next".into(),
            title: "Clause moat".into(),
            preview: "Drive the thesis".into(),
            current: false,
        }]);
        let picker = render_text(&mut app, 100, 18);
        assert!(picker.contains("Resume Conversation"));
        assert!(picker.contains("Clause moat"));
        assert!(picker.contains("Drive the thesis"));

        app.picker = None;
        app.load_conversation(codex::ConversationSnapshot {
            id: "thread-next".into(),
            model: "gpt-5.6-terra".into(),
            reasoning_effort: "high".into(),
            entries: vec![
                codex::ConversationEntry::Operator("new thread prompt".into()),
                codex::ConversationEntry::Command(codex::CommandOutcome {
                    command: "cargo test".into(),
                    succeeded: true,
                }),
                codex::ConversationEntry::Agent("new thread answer".into()),
            ],
        });
        let replay = render_text(&mut app, 100, 18);
        assert!(replay.contains("new thread prompt"));
        assert!(replay.contains("Ran cargo test"));
        assert!(replay.contains("new thread answer"));
        assert!(!replay.contains("FIRST"));
        assert!(!replay.contains("first answer"));
        assert_eq!(app.model, "gpt-5.6-terra");
        assert_eq!(app.reasoning_effort, "high");
    }

    #[test]
    fn model_picker_renders_the_codex_style_model_then_effort_flow() {
        let models = vec![
            codex::ModelOption {
                model: "gpt-5.6-sol".into(),
                description: "Latest frontier agentic coding model.".into(),
                reasoning: vec![
                    codex::ReasoningOption {
                        effort: "low".into(),
                        description: "Fast responses with lighter reasoning".into(),
                    },
                    codex::ReasoningOption {
                        effort: "high".into(),
                        description: "Greater reasoning depth for complex problems".into(),
                    },
                    codex::ReasoningOption {
                        effort: "max".into(),
                        description: "For difficult problems".into(),
                    },
                ],
                default_effort: "low".into(),
                is_default: true,
            },
            codex::ModelOption {
                model: "gpt-5.6-terra".into(),
                description: "Balanced agentic coding model for everyday work.".into(),
                reasoning: vec![],
                default_effort: "medium".into(),
                is_default: false,
            },
        ];
        let mut app = accepted_frame_app();
        app.model = "gpt-5.6-sol".into();
        app.reasoning_effort = "high".into();
        app.picker = Picker::models(models.clone(), &app.model);

        let model_view = render_text(&mut app, 110, 18);
        assert!(model_view.contains("Select Model and Effort"));
        assert!(model_view.contains("gpt-5.6-sol (current)"));
        assert!(model_view.contains("gpt-5.6-terra"));

        app.picker = Some(Picker::efforts(
            models,
            0,
            &app.model,
            &app.reasoning_effort,
            true,
        ));
        let effort_view = render_text(&mut app, 110, 18);
        assert!(effort_view.contains("Select Reasoning Level for gpt-5.6-sol"));
        assert!(effort_view.contains("High (current)"));
        assert!(effort_view.contains("More reasoning…"));
        assert!(effort_view.contains("Max consumes usage limits faster"));
    }

    #[test]
    fn switchboard_renders_authority_resolved_units_and_controls() {
        let mut app = accepted_frame_app();
        app.picker = Some(Picker::switchboard(vec![
            agent_catalog::ActivationUnit {
                id: "worktree-guard".into(),
                kind: "hook".into(),
                active: true,
                description: "Protect worktrees".into(),
                source: PathBuf::from("/tmp/worktree-guard.sh"),
            },
            agent_catalog::ActivationUnit {
                id: "planning".into(),
                kind: "module".into(),
                active: false,
                description: "Planning".into(),
                source: PathBuf::from("/tmp/catalog.json"),
            },
            reference_unit("agent-policy-distilled", "Agent Policy Distilled"),
        ]));

        let rendered = render_text(&mut app, 100, 18);
        assert!(rendered.contains("Switchboard"));
        assert!(rendered.contains("space toggle · enter edit · @ reference"));
        assert!(rendered.contains("Name"));
        assert!(rendered.contains("Status"));
        assert!(rendered.contains("HOOK"));
        assert!(rendered.contains("MODULE"));
        let compact = rendered
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(compact.contains("worktree-guard on"));
        assert!(compact.contains("planning off"));
        assert!(compact.contains("agent-policy-distilled on"));
        assert!(!rendered.contains("Agent Policy Distilled"));
        assert_eq!(rendered.matches("agent-policy-distilled").count(), 1);
    }

    fn reference_unit(id: &str, description: &str) -> agent_catalog::ActivationUnit {
        agent_catalog::ActivationUnit {
            id: id.into(),
            kind: "skill".into(),
            active: true,
            description: description.into(),
            source: PathBuf::from(format!("/tmp/skills/{id}/SKILL.md")),
        }
    }

    #[test]
    fn switchboard_reference_keeps_the_draft_and_inserts_the_owning_source() {
        let mut app = accepted_frame_app();
        app.picker = Some(Picker::switchboard(vec![reference_unit(
            "agent-policy-distilled",
            "Author agent policy",
        )]));
        app.reference_switchboard_selection();
        assert!(app.picker.is_none());
        assert_eq!(
            app.composer.text(),
            "next question\n@agent-policy-distilled (skill source: /tmp/skills/agent-policy-distilled/SKILL.md) "
        );
        assert!(!app.is_working());
    }

    #[test]
    fn reference_menu_filters_renders_and_inserts_without_submitting() {
        let mut app = accepted_frame_app();
        app.composer.replace_text("Please use @policy");
        app.reference_candidates = Some(vec![
            reference_unit("agent-policy-distilled", "Author agent policy"),
            reference_unit("agent-policy-reference", "Detailed policy notes"),
            reference_unit("threejs-animation-distilled", "Animate objects"),
        ].into_iter().map(Into::into).collect());
        app.refresh_reference_menu();
        let rendered = render_text(&mut app, 130, 22);
        for expected in [
            "Name",
            "Description",
            "Type",
            "Author agent policy",
            "agent-policy-reference",
        ] {
            assert!(rendered.contains(expected), "missing {expected}");
        }
        assert!(!rendered.contains("threejs-animation-distilled"));
        assert!(
            app.handle_reference_key(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
        );
        assert_eq!(app.state.reference_selection(), 1);
        assert!(
            app.handle_reference_key(&KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL))
        );
        assert_eq!(app.state.reference_selection(), 0);
        assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(
            app.composer.text(),
            "Please use @agent-policy-reference (skill source: /tmp/skills/agent-policy-reference/SKILL.md) "
        );
        assert!(!app.is_working());
    }

    #[test]
    fn escape_closes_references_and_typing_reopens_them() {
        let mut app = accepted_frame_app();
        app.composer.replace_text("@policy");
        app.reference_candidates = Some(vec![reference_unit("agent-policy-distilled", "Policy").into()]);
        app.refresh_reference_menu();
        assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(app.reference_query().is_none());
        assert_eq!(app.composer.text(), "@policy");
        app.composer.insert_text("-d");
        app.refresh_reference_menu();
        assert_eq!(app.reference_query().as_deref(), Some("policy-d"));
    }

    #[test]
    fn project_reference_filters_and_inserts_the_exact_path_without_submitting() {
        let mut app = accepted_frame_app();
        app.composer.replace_text("Inspect @ång");
        app.reference_candidates = Some(vec![
            references::Reference {name: "src/Ångström notes.rs".into(), description: "Project file".into(), kind: "file".into(), path: "/tmp/project/src/Ångström notes.rs".into()},
            reference_unit("agent-policy-distilled", "Policy").into(),
        ]);
        app.refresh_reference_menu();
        let rendered = render_text(&mut app, 130, 22);
        assert!(rendered.contains("src/Ångström notes.rs"), "{rendered}");
        assert!(rendered.contains("File"));
        assert!(!rendered.contains("agent-policy-distilled"));
        assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(app.composer.text(), "Inspect @src/Ångström notes.rs (file source: /tmp/project/src/Ångström notes.rs) ");
        assert!(!app.is_working());
        assert!(app.reference_query().is_none());
    }

    #[test]
    fn editor_receives_the_source_as_one_literal_argument() {
        let path = Path::new("/tmp/a skill; $(printf unexpected)/SKILL.md");
        let output = editor_command("printf '%s\\n'", path).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\n", path.display())
        );
    }

    #[test]
    fn transcript_keeps_the_newest_turn_visible() {
        let mut app = accepted_frame_app();
        app.transcript = (0..20)
            .map(|index| (Speaker::North, format!("answer {index}")))
            .collect();

        let rendered = render_text(&mut app, 80, 10);
        assert!(rendered.contains("• answer 19"));
        assert!(!rendered.contains("• answer 0 "));
    }

    #[test]
    fn empty_chat_view_is_a_truthful_welcome_card() {
        let mut app = App::open(PathBuf::from("/home/tom/demo")).unwrap();
        app.model = "gpt-5.6-sol".into();
        app.reasoning_effort = "low".into();

        let rendered = render_text(&mut app, 100, 15);

        assert!(rendered.contains("North (v0.1.0)"));
        assert!(rendered.contains("model:       gpt-5.6-sol low"));
        assert!(rendered.contains("directory:   /home/tom/demo"));
        assert!(rendered.contains("permissions: workspace write; approvals follow your settings"));
        assert!(rendered.contains("› Main · / commands"));
        assert!(!rendered.contains("Main (ready)"));
        assert!(!rendered.contains("· ready ·"));
    }

    #[test]
    fn conversation_replaces_the_welcome_card_and_highlights_operator_turns() {
        let mut app = accepted_frame_app();
        let mut terminal = Terminal::new(TestBackend::new(100, 15)).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(!render_text(&mut app, 100, 15).contains("North (v0.1.0)"));
        assert_eq!(buffer[(1, 1)].bg, Color::Rgb(52, 58, 62));
        assert_eq!(buffer[(40, 1)].bg, Color::Rgb(52, 58, 62));
    }

    #[test]
    fn agent_responses_render_the_complete_commonmark_and_gfm_surface() {
        let mut app = accepted_frame_app();
        app.transcript = vec![(
            Speaker::North,
            concat!(
                "---\ntitle: Complete fixture\n---\n\n",
                "# Heading {#heading}\n\n",
                "**bold** *emphasis* ~~deleted~~ `inline code`  \n",
                "hard break and <https://openai.com>.\n\n",
                "- plain item\n- [x] completed task\n- [ ] pending task\n\n",
                "1. ordered item\n2. second item\n\n",
                "> quoted text\n\n",
                "---\n\n",
                "```rust\nfn main() {}\n```\n\n",
                "[OpenAI](https://openai.com) and ![diagram](diagram.png)\n\n",
                "Inline <kbd>HTML</kbd> remains visible.\n\n",
                "| A | B |\n|:--|--:|\n| 1 | 2 |\n\n",
                "Term\n: definition text\n\n",
                "Footnote reference[^note].\n\n[^note]: Footnote body.\n\n",
                "Inline math $x + y$, display math $$z = 3$$, H ~2~ O, and x ^2^."
            )
            .into(),
        )];

        let text = conversation_text(&app, 100);
        let rendered = text.to_string();
        for expected in [
            "Heading",
            "bold",
            "emphasis",
            "deleted",
            "inline code",
            "hard break",
            "- [x] completed task",
            "1. ordered item",
            "quoted text",
            "fn main() {}",
            "OpenAI",
            "https://openai.com",
            "[img] diagram (diagram.png)",
            "<kbd>HTML</kbd>",
            "definition text",
            "Footnote body",
            "$x + y$",
            "$$z = 3$$",
        ] {
            assert!(
                rendered.contains(expected),
                "missing rendered markdown: {expected}"
            );
        }
        assert!(rendered.contains('┌'));
        for source_marker in ["**bold**", "~~deleted~~", "`inline code`", "![diagram]"] {
            assert!(!rendered.contains(source_marker));
        }
        assert!(text.lines.iter().flat_map(|line| &line.spans).any(|span| {
            span.content.contains("bold") && span.style.add_modifier.contains(Modifier::BOLD)
        }));
    }

    #[tokio::test]
    async fn working_row_follows_the_submitted_message_inside_the_transcript() {
        let mut app = accepted_frame_app();
        app.transcript = vec![(Speaker::Operator, "current question".into())];
        app.composer = Composer::new();
        hold_turn(&mut app);

        let rendered = render_text(&mut app, 80, 12);
        let rows = rendered.lines().collect::<Vec<_>>();
        let message_row = rows
            .iter()
            .position(|row| row.contains("› current question"))
            .unwrap();
        let working_row = rows
            .iter()
            .position(|row| row.contains("• Working (0s • esc to interrupt)"))
            .unwrap();
        let composer_row = rows.iter().position(|row| row.contains("❯ ")).unwrap();

        assert_eq!(working_row, message_row + 2);
        assert!(working_row < composer_row);
        app.shutdown().await;
    }

    #[test]
    fn working_elapsed_time_matches_codex_compact_format() {
        assert_eq!(fmt_elapsed_compact(9), "9s");
        assert_eq!(fmt_elapsed_compact(60), "1m 00s");
        assert_eq!(fmt_elapsed_compact(62), "1m 02s");
        assert_eq!(fmt_elapsed_compact(3661), "1h 01m 01s");

        let app = accepted_frame_app();
        let line = working_line(&app).to_string();
        assert!(line.starts_with("• Working (0s • esc to interrupt)"));
    }

    #[test]
    fn interruption_is_neutral_and_does_not_leave_a_failed_footer() {
        let mut app = accepted_frame_app();
        app.status = "idle".into();
        app.transcript = vec![(Speaker::Notice, "Interrupted".into())];

        let rendered = render_text(&mut app, 80, 10);
        assert!(rendered.contains("• Interrupted"));
        assert!(rendered.contains("› Main · / commands"));
        assert!(!rendered.contains("· failed"));
    }

    #[test]
    fn command_results_use_green_and_red_dots() {
        let mut app = accepted_frame_app();
        app.transcript = vec![
            (Speaker::CommandSuccess, "cargo test".into()),
            (Speaker::CommandFailure, "cargo build".into()),
        ];
        let text = conversation_text(&app, 80);

        assert_eq!(text.lines[0].to_string(), "• Ran cargo test");
        assert_eq!(text.lines[2].to_string(), "• Ran cargo build");
        assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::Green));
        assert_eq!(text.lines[2].spans[0].style.fg, Some(Color::Red));
    }
}
