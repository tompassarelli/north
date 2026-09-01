mod agent_catalog;
mod clause_state;
mod codex;
mod error;

use std::env;
use std::io::{self, Stdout};
use std::path::PathBuf;

use clause_state::{NorthPhase, NorthState};
use codex::Codex;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use error::{NorthError, NorthResult};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
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
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

struct App {
    cwd: PathBuf,
    state: NorthState,
    codex: Option<Codex>,
    input: String,
    transcript: Vec<(Speaker, String)>,
    status: String,
}

#[derive(Clone, Copy)]
enum Speaker {
    Operator,
    North,
    System,
}

impl App {
    fn open(cwd: PathBuf) -> NorthResult<Self> {
        let state = NorthState::open()?;
        Ok(Self {
            cwd,
            state,
            codex: None,
            input: String::new(),
            transcript: Vec::new(),
            status: "ready".into(),
        })
    }

    async fn submit(&mut self, prompt: String) {
        self.transcript.push((Speaker::Operator, prompt.clone()));
        match delegation_task(&prompt) {
            Ok(Some(task)) => self.submit_delegation(task.to_owned()).await,
            Ok(None) => self.submit_direct(prompt).await,
            Err(error) => self.record_error(error),
        }
    }

    async fn submit_direct(&mut self, prompt: String) {
        if let Err(error) = self.state.submit() {
            self.record_error(error);
            return;
        }
        self.status = "starting Codex".into();
        if let Err(error) = self.ensure_codex().await {
            self.settle_direct_failure();
            self.record_error(error);
            return;
        }
        self.status = "Codex is working".into();
        let result = match self.codex.as_mut() {
            Some(codex) => codex.run_turn(&prompt).await,
            None => Err(NorthError::Protocol("Codex client disappeared".into())),
        };
        match result {
            Ok(answer) => {
                if let Err(error) = self.state.settle_success() {
                    self.record_error(error);
                    return;
                }
                self.transcript.push((Speaker::North, answer));
                self.status = "complete".into();
            }
            Err(error) => {
                self.settle_direct_failure();
                self.record_error(error);
            }
        }
    }

    async fn submit_delegation(&mut self, task: String) {
        if let Err(error) = self.state.delegate() {
            self.record_error(error);
            return;
        }
        self.status = "starting delegation coordinator".into();
        if let Err(error) = self.ensure_codex().await {
            self.settle_delegation_failure();
            self.record_error(error);
            return;
        }

        self.status = "delegating through Codex".into();
        let result = {
            let (state, codex) = (&mut self.state, &mut self.codex);
            match codex.as_mut() {
                Some(codex) => {
                    codex
                        .run_delegate(&task, |child_id| state.child_spawned(child_id))
                        .await
                }
                None => Err(NorthError::Protocol("Codex client disappeared".into())),
            }
        };
        match result {
            Ok(outcome) => {
                if let Err(error) = self.state.settle_delegation_success(&outcome.child_id) {
                    self.record_error(error);
                    return;
                }
                self.transcript.push((Speaker::North, outcome.answer));
                self.status = "complete".into();
            }
            Err(error) => {
                self.settle_delegation_failure();
                self.record_error(error);
            }
        }
    }

    async fn ensure_codex(&mut self) -> NorthResult<()> {
        if self.codex.is_none() {
            self.codex = Some(Codex::start(&self.cwd).await?);
        }
        Ok(())
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
        self.transcript.push((Speaker::System, error.to_string()));
    }

    async fn shutdown(&mut self) {
        if let Some(codex) = self.codex.take() {
            if let Err(error) = codex.shutdown().await {
                self.record_error(error);
            }
        }
    }
}

fn delegation_task(prompt: &str) -> NorthResult<Option<&str>> {
    let Some(remainder) = prompt.strip_prefix("/delegate") else {
        return Ok(None);
    };
    if !remainder.is_empty() && !remainder.chars().next().is_some_and(char::is_whitespace) {
        return Ok(None);
    }
    let task = remainder.trim_start();
    if task.is_empty() {
        return Err(NorthError::Protocol(
            "/delegate requires one explicit task".into(),
        ));
    }
    Ok(Some(task))
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
        draw(terminal, app)?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        match key.code {
            KeyCode::Char(character) => app.input.push(character),
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Enter => {
                let prompt = std::mem::take(&mut app.input);
                if prompt == "/q" {
                    break;
                }
                if prompt.is_empty() {
                    continue;
                }
                app.status = "authorizing in Clause".into();
                draw(terminal, app)?;
                app.submit(prompt).await;
            }
            _ => {}
        }
    }
    Ok(())
}

fn draw(terminal: &mut NorthTerminal, app: &App) -> NorthResult<()> {
    terminal.draw(|frame| {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let header = Paragraph::new(format!(
            "North TUI  |  {}  |  Clause: {}",
            app.status,
            app.state.phase().label()
        ))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(header, rows[0]);

        let mut lines = Vec::new();
        for (speaker, message) in &app.transcript {
            let (label, style) = match speaker {
                Speaker::Operator => ("you", Style::default().fg(Color::Yellow)),
                Speaker::North => ("north", Style::default().fg(Color::Green)),
                Speaker::System => ("system", Style::default().fg(Color::Red)),
            };
            lines.push(Line::styled(format!("{label}> {message}"), style));
        }
        let transcript = Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Conversation").borders(Borders::ALL));
        frame.render_widget(transcript, rows[1]);

        let input = Paragraph::new(app.input.as_str()).block(
            Block::default()
                .title("Prompt (/q to quit)")
                .borders(Borders::ALL),
        );
        frame.render_widget(input, rows[2]);
        let cursor_x = rows[2].x + 1 + app.input.chars().count() as u16;
        frame.set_cursor_position((
            cursor_x.min(rows[2].right().saturating_sub(2)),
            rows[2].y + 1,
        ));
    })?;
    Ok(())
}
