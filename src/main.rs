mod agent_catalog;
mod clause_state;
mod codex;
mod command_surface;
mod composer;
mod error;

use std::env;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use clause_state::{AttachmentIdentity, NorthPhase, NorthState};
use codex::{Codex, ConversationEntry, ConversationSnapshot};
use command_surface::{Picker, matching_commands, render_picker, render_slash_menu};
use composer::{Composer, Submission};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
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
    Agents(Vec<String>),
}

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<NorthCommand> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(NorthCommand::Tui);
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(NorthCommand::Agents(arguments[2..].to_vec()));
    }
    Err(NorthError::Configuration(
        "usage: north [config agents {sync|status|on|off|path|inspect} ...]".into(),
    ))
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
    model: String,
    reasoning_effort: String,
    composer: Composer,
    transcript: Vec<(Speaker, String)>,
    status: String,
    turn: Option<JoinHandle<TurnCompletion>>,
    interrupt: Option<oneshot::Sender<()>>,
    turn_started_at: Option<Instant>,
    picker: Option<Picker>,
    command_index: usize,
}

struct TurnCompletion {
    codex: Codex,
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
            model: "Codex default".into(),
            reasoning_effort: "default".into(),
            composer: Composer::new(),
            transcript: Vec::new(),
            status: "idle".into(),
            turn: None,
            interrupt: None,
            turn_started_at: None,
            picker: None,
            command_index: 0,
        })
    }

    async fn accept_submission(&mut self, mut submission: Submission) -> bool {
        let input = submission.text.clone();
        let command = input.trim();
        let is_command = command.starts_with('/');
        let previous_notice = self.state.notice().to_owned();
        let transition = if is_command {
            self.state.execute_command(command)
        } else {
            self.state.submit_input(&input)
        };
        if let Err(error) = transition {
            self.detach_images(submission.attachment_identities());
            self.record_error(error);
            return false;
        }
        if !is_command {
            self.transcript.push((Speaker::Operator, input));
        }
        let notice = self.state.notice();
        if notice != previous_notice && !notice.is_empty() {
            self.transcript.push((Speaker::Notice, notice.to_owned()));
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
            "new-conversation" => {
                self.detach_images(submission.attachment_identities());
                self.new_conversation().await;
                false
            }
            "resume-conversation" => {
                self.detach_images(submission.attachment_identities());
                self.open_conversation_picker().await;
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
        let Some(mut codex) = self.codex.take() else {
            self.settle_direct_failure();
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        self.status = "working".into();
        self.turn_started_at = Some(Instant::now());
        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        self.interrupt = Some(interrupt_tx);
        self.turn = Some(tokio::spawn(async move {
            let result = codex
                .run_turn_interruptible(&submission.text, &image_paths, interrupt_rx)
                .await;
            TurnCompletion {
                codex,
                result: TurnResult::Direct(result),
            }
        }));
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
        let Some(mut codex) = self.codex.take() else {
            self.settle_delegation_failure();
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        self.status = "working".into();
        self.turn_started_at = Some(Instant::now());
        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        self.interrupt = Some(interrupt_tx);
        self.turn = Some(tokio::spawn(async move {
            let mut child_id = None;
            let result = codex
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
            TurnCompletion {
                codex,
                result: TurnResult::Delegation { child_id, result },
            }
        }));
    }

    fn detach_images(&mut self, identities: Vec<AttachmentIdentity>) {
        for identity in identities {
            if let Err(error) = self.state.detach_image(identity) {
                self.record_error(error);
            }
        }
    }

    fn is_working(&self) -> bool {
        self.turn.is_some()
    }

    async fn collect_finished_turn(&mut self) {
        if !self.turn.as_ref().is_some_and(|turn| turn.is_finished()) {
            return;
        }
        let Some(turn) = self.turn.take() else {
            return;
        };
        self.interrupt = None;
        self.turn_started_at = None;
        let completion = match turn.await {
            Ok(completion) => completion,
            Err(error) => {
                self.settle_active_failure();
                self.record_error(NorthError::Protocol(format!(
                    "background turn stopped unexpectedly: {error}"
                )));
                return;
            }
        };
        self.codex = Some(completion.codex);
        match completion.result {
            TurnResult::Direct(result) => self.finish_direct(result),
            TurnResult::Delegation { child_id, result } => self.finish_delegation(child_id, result),
        }
    }

    fn finish_direct(&mut self, result: NorthResult<codex::TurnOutcome>) {
        match result {
            Ok(outcome) => match self.state.settle_success() {
                Ok(()) => {
                    for command in outcome.commands {
                        let speaker = if command.succeeded {
                            Speaker::CommandSuccess
                        } else {
                            Speaker::CommandFailure
                        };
                        self.transcript.push((speaker, command.command));
                    }
                    self.transcript.push((Speaker::North, outcome.answer));
                    self.status = "complete".into();
                }
                Err(error) => self.record_error(error),
            },
            Err(NorthError::Interrupted) => {
                self.settle_direct_failure();
                self.record_interruption();
            }
            Err(error) => {
                self.settle_direct_failure();
                self.record_error(error);
            }
        }
    }

    fn finish_delegation(
        &mut self,
        child_id: Option<String>,
        result: NorthResult<codex::DelegationOutcome>,
    ) {
        if let Some(child_id) = child_id.as_deref()
            && let Err(error) = self.state.child_spawned(child_id)
        {
            self.record_error(error);
            return;
        }
        match result {
            Ok(outcome) => match self.state.settle_delegation_success(&outcome.child_id) {
                Ok(()) => {
                    self.transcript.push((Speaker::North, outcome.answer));
                    self.status = "complete".into();
                }
                Err(error) => self.record_error(error),
            },
            Err(NorthError::Interrupted) => {
                self.settle_delegation_failure();
                self.record_interruption();
            }
            Err(error) => {
                self.settle_delegation_failure();
                self.record_error(error);
            }
        }
    }

    fn settle_active_failure(&mut self) {
        match self.state.phase() {
            NorthPhase::Dispatching => self.settle_direct_failure(),
            NorthPhase::Delegating | NorthPhase::Settling => self.settle_delegation_failure(),
            _ => {}
        }
    }

    fn interrupt_turn(&mut self) {
        if let Some(interrupt) = self.interrupt.take() {
            let _ = interrupt.send(());
        }
    }

    async fn ensure_codex(&mut self) -> NorthResult<()> {
        if self.codex.is_none() {
            let mut codex = Codex::connect(&self.cwd).await?;
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
            self.codex = Some(codex);
        }
        Ok(())
    }

    async fn new_conversation(&mut self) {
        if self.is_working() {
            self.record_error(NorthError::Protocol(
                "Interrupt the active response before starting a new conversation".into(),
            ));
            return;
        }
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
                    self.transcript.clear();
                    self.model = codex.model().to_owned();
                    self.reasoning_effort = codex.reasoning_effort().to_owned();
                    self.status = "idle".into();
                }
                Err(error) => self.record_error(error),
            },
            Err(error) => {
                let _ = self.state.fail_new_conversation();
                self.record_error(error);
            }
        }
    }

    async fn open_conversation_picker(&mut self) {
        if self.is_working() {
            self.record_error(NorthError::Protocol(
                "Interrupt the active response before switching conversations".into(),
            ));
            return;
        }
        let Some(codex) = self.codex.as_mut() else {
            self.record_error(NorthError::Protocol("Codex client is unavailable".into()));
            return;
        };
        match codex.conversations(&self.cwd).await {
            Ok(conversations) => {
                for conversation in &conversations {
                    if let Err(error) = self.state.observe_conversation(&conversation.id) {
                        self.record_error(error);
                        return;
                    }
                }
                self.picker = Picker::conversations(conversations);
                if self.picker.is_none() {
                    self.transcript.push((
                        Speaker::Notice,
                        "No previous conversations in this directory".into(),
                    ));
                }
            }
            Err(error) => self.record_error(error),
        }
    }

    async fn switch_conversation(&mut self, conversation_id: &str) {
        if self.state.active_conversation() == Some(conversation_id) {
            return;
        }
        if let Err(error) = self.state.request_switch_conversation(conversation_id) {
            self.record_error(error);
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
                    self.load_conversation(snapshot);
                    self.status = "idle".into();
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
        self.model = snapshot.model;
        self.reasoning_effort = snapshot.reasoning_effort;
        self.transcript = snapshot
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
            .collect();
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
                let index = units.iter().position(|unit| unit.id == id).unwrap_or(0);
                self.picker = Some(Picker::Switchboard { units, index });
            }
            Err(error) => self.record_error(error),
        }
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
                self.status = "idle".into();
                self.transcript.push((
                    Speaker::Notice,
                    format!("Using {model} · {effort} reasoning"),
                ));
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
        self.transcript
            .push((Speaker::System, error.user_message()));
    }

    fn record_interruption(&mut self) {
        self.status = "idle".into();
        self.transcript
            .push((Speaker::Notice, "Interrupted".into()));
    }

    async fn shutdown(&mut self) {
        if let Some(turn) = self.turn.take() {
            self.interrupt = None;
            self.turn_started_at = None;
            turn.abort();
            let _ = turn.await;
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
async fn main() -> NorthResult<()> {
    match parse_command(env::args().skip(1))? {
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
        assert!(parse_command(["config", "unknown"].map(str::to_owned)).is_err());
    }
}

async fn run(terminal: &mut NorthTerminal, app: &mut App) -> NorthResult<()> {
    loop {
        app.collect_finished_turn().await;
        draw(terminal, app)?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let terminal_event = event::read()?;
        if let Event::Paste(pasted) = terminal_event {
            app.composer.insert_text(&pasted.replace('\r', "\n"));
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
        if app.picker.is_some() {
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
                KeyCode::Enter => app.accept_picker_selection().await,
                _ => {}
            }
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
                    app.transcript.push((
                        Speaker::System,
                        format!("Could not paste clipboard image: {error}"),
                    ));
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
        match key.code {
            KeyCode::Enter => {
                if app.composer.is_empty() || app.is_working() {
                    continue;
                }
                let submission = app.composer.take_submission();
                if app.accept_submission(submission).await {
                    break;
                }
            }
            _ => {
                app.command_index = 0;
                let removed = app.composer.handle_key(key);
                app.detach_images(removed);
            }
        }
    }
    Ok(())
}

fn navigate_view(state: &mut NorthState, key: &KeyCode, composer_empty: bool) -> NorthResult<bool> {
    match key {
        KeyCode::Tab => state.navigate_view(true)?,
        KeyCode::BackTab => state.navigate_view(false)?,
        KeyCode::Right if composer_empty => state.navigate_view(true)?,
        KeyCode::Left if composer_empty => state.navigate_view(false)?,
        KeyCode::Esc if state.active_view() != "agents" => state.execute_command("/agents")?,
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

    if let Some(picker) = app.picker.as_ref() {
        render_picker(frame, rows[0], picker, &app.model, &app.reasoning_effort);
    } else {
        match app.state.active_view() {
            "agents" => {
                if app.transcript.is_empty() {
                    render_welcome(frame, rows[0], app);
                } else {
                    let width = usize::from(rows[0].width.max(1));
                    let transcript = conversation_text(app, width);
                    let line_count = transcript
                        .lines
                        .iter()
                        .map(|line| line.width().max(1).div_ceil(width))
                        .sum::<usize>();
                    let hidden_lines = line_count
                        .saturating_sub(rows[0].height as usize)
                        .min(u16::MAX as usize) as u16;
                    frame.render_widget(
                        Paragraph::new(transcript)
                            .wrap(Wrap { trim: false })
                            .scroll((hidden_lines, 0)),
                        rows[0],
                    );
                }
            }
            "goals" => frame.render_widget(
                Paragraph::new(goals_text(&app.state)).wrap(Wrap { trim: false }),
                rows[0],
            ),
            "all" => frame.render_widget(
                Paragraph::new(all_text(&app.state)).wrap(Wrap { trim: false }),
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
    if app.picker.is_none() {
        render_slash_menu(
            frame,
            rows[1],
            app.state.commands(),
            &app.composer.text(),
            app.command_index,
        );
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
        "agents" => format!(
            "{} {} · {} · {}",
            app.model,
            app.reasoning_effort,
            app.cwd.display(),
            app.branch
        ),
        "goals" => "desired outcomes".into(),
        "all" => "all tracked things".into(),
        other => other.into(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Agents", tab_style("agents")),
            Span::styled(" | ", inactive_tab_style),
            Span::styled("Goals", tab_style("goals")),
            Span::styled(" | ", inactive_tab_style),
            Span::styled("All", tab_style("all")),
            Span::styled(" > ", inactive_tab_style),
            Span::raw(view_context),
        ])),
        rows[2],
    );

    let mut footer = vec![
        Span::styled("› ", Style::default().fg(Color::Cyan)),
        Span::styled("Main", Style::default().add_modifier(Modifier::BOLD)),
    ];
    if app.status == "failed" {
        footer.extend([
            Span::raw(" · "),
            Span::styled("failed", Style::default().fg(Color::Red)),
        ]);
    }
    footer.push(Span::styled(
        " · / commands",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(footer)), rows[3]);
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
            welcome_field("permissions:", "workspace write, no approval prompts"),
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

fn all_text(state: &NorthState) -> Text<'static> {
    if state.goals().is_empty() {
        Text::from("No tracked things")
    } else {
        goals_text(state)
    }
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
            Speaker::CommandSuccess | Speaker::CommandFailure => {
                let color = if matches!(speaker, Speaker::CommandSuccess) {
                    Color::Green
                } else {
                    Color::Red
                };
                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(color)),
                    Span::styled("Ran ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(message),
                ]));
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
        .turn_started_at
        .map(|started| started.elapsed())
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
    async fn clause_drives_the_tui_goal_create_inspect_redirect_inspect_journey() {
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

        assert!(!app.accept_submission(submission("/redirect")).await);
        assert_eq!(app.state.notice(), "Describe the new desired outcome");
        assert!(
            !app.accept_submission(submission("Make Clause own the whole TUI"))
                .await
        );
        let redirected = render_text(&mut app, 100, 20);
        assert!(redirected.contains("Make Clause own the whole TUI"));
        assert!(redirected.contains("Previous objectives"));
        assert!(redirected.contains("Make Clause own Goals"));
    }

    #[test]
    fn headless_surface_matches_the_accepted_product_frame() {
        let mut app = accepted_frame_app();
        let rendered = render_text(&mut app, 110, 12);

        assert!(
            rendered.contains(
                "Agents | Goals | All > gpt-example high · /tmp/demo · north-v2-usable-tui"
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
    fn tab_and_arrow_keys_navigate_the_three_product_views() {
        let mut state = NorthState::open().expect("North Clause source opens");

        assert!(navigate_view(&mut state, &KeyCode::Tab, true).unwrap());
        assert_eq!(state.active_view(), "goals");
        assert!(navigate_view(&mut state, &KeyCode::Right, true).unwrap());
        assert_eq!(state.active_view(), "all");
        assert!(navigate_view(&mut state, &KeyCode::Tab, true).unwrap());
        assert_eq!(state.active_view(), "agents");
        assert!(navigate_view(&mut state, &KeyCode::Left, true).unwrap());
        assert_eq!(state.active_view(), "all");
        assert!(navigate_view(&mut state, &KeyCode::BackTab, true).unwrap());
        assert_eq!(state.active_view(), "goals");
        assert!(navigate_view(&mut state, &KeyCode::Esc, true).unwrap());
        assert_eq!(state.active_view(), "agents");
        assert!(!navigate_view(&mut state, &KeyCode::Up, true).unwrap());
        assert!(!navigate_view(&mut state, &KeyCode::Left, false).unwrap());
    }

    #[test]
    fn goals_and_all_render_their_established_empty_states() {
        let mut app = accepted_frame_app();

        app.state.execute_command("/goals").unwrap();
        let goals = render_text(&mut app, 110, 12);
        assert!(goals.contains("Agents | Goals | All > desired outcomes"));
        assert!(goals.contains("No Goals"));
        assert!(!goals.contains("first answer"));

        app.state.execute_command("/all").unwrap();
        let all = render_text(&mut app, 110, 12);
        assert!(all.contains("Agents | Goals | All > all tracked things"));
        assert!(all.contains("No tracked things"));
        assert!(!all.contains("first answer"));
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
                detail: "supports repo-safety-distilled".into(),
            },
            agent_catalog::ActivationUnit {
                id: "planning".into(),
                kind: "module".into(),
                active: false,
                detail: "3 members".into(),
            },
        ]));

        let rendered = render_text(&mut app, 100, 18);
        assert!(rendered.contains("Switchboard"));
        assert!(rendered.contains("↑/↓ move · space toggle · esc close"));
        assert!(rendered.contains("HOOK"));
        assert!(rendered.contains("worktree-guard: on"));
        assert!(rendered.contains("MODULE"));
        assert!(rendered.contains("planning: off · 3 members"));
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
    fn empty_agents_view_is_a_truthful_welcome_card() {
        let mut app = App::open(PathBuf::from("/home/tom/demo")).unwrap();
        app.model = "gpt-5.6-sol".into();
        app.reasoning_effort = "low".into();

        let rendered = render_text(&mut app, 100, 15);

        assert!(rendered.contains("North (v0.1.0)"));
        assert!(rendered.contains("model:       gpt-5.6-sol low"));
        assert!(rendered.contains("directory:   /home/tom/demo"));
        assert!(rendered.contains("permissions: workspace write, no approval prompts"));
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
        app.turn_started_at = Some(Instant::now());
        app.turn = Some(tokio::spawn(std::future::pending::<TurnCompletion>()));

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
        app.turn.take().unwrap().abort();
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
