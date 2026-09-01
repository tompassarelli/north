mod agent_catalog;
mod clause_state;
mod codex;
mod error;

use std::env;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;

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
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, layout::Rect};

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
    branch: String,
    state: NorthState,
    codex: Option<Codex>,
    model: String,
    reasoning_effort: String,
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
        let branch = session_branch(&cwd);
        Ok(Self {
            cwd,
            branch,
            state,
            codex: None,
            model: "Codex default".into(),
            reasoning_effort: "default".into(),
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
            let codex = Codex::start(&self.cwd).await?;
            self.model = codex.model().to_owned();
            self.reasoning_effort = codex.reasoning_effort().to_owned();
            self.codex = Some(codex);
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
    terminal.draw(|frame| render(frame, app))?;
    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let area = padded(frame.area());
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let transcript_width = rows[0].width.max(1) as usize;
    let transcript_line_count = app
        .transcript
        .iter()
        .map(|(speaker, message)| {
            let prefix_width = match speaker {
                Speaker::Operator => "you> ".len(),
                Speaker::North => "north> ".len(),
                Speaker::System => "system> ".len(),
            };
            message
                .lines()
                .map(|line| {
                    (prefix_width + line.chars().count())
                        .max(1)
                        .div_ceil(transcript_width)
                })
                .sum::<usize>()
                .max(1)
        })
        .sum::<usize>();
    let transcript = app
        .transcript
        .iter()
        .map(|(speaker, message)| {
            let (label, color) = match speaker {
                Speaker::Operator => ("you", Color::Yellow),
                Speaker::North => ("north", Color::Green),
                Speaker::System => ("system", Color::Red),
            };
            Line::from(vec![
                Span::styled(
                    format!("{label}> "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(message),
            ])
        })
        .collect::<Vec<_>>();
    let transcript = Paragraph::new(Text::from(transcript)).wrap(Wrap { trim: false });
    let hidden_lines = transcript_line_count
        .saturating_sub(rows[0].height as usize)
        .min(u16::MAX as usize) as u16;
    frame.render_widget(transcript.scroll((hidden_lines, 0)), rows[0]);

    let composer_style = Style::default()
        .fg(Color::Rgb(229, 231, 235))
        .bg(Color::Rgb(37, 39, 45));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Color::Cyan)),
            Span::styled(app.input.as_str(), composer_style),
        ]))
        .style(composer_style),
        rows[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Agents",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" | Goals | All > ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "{} {} · {} · {}",
                app.model,
                app.reasoning_effort,
                app.cwd.display(),
                app.branch
            )),
        ])),
        rows[2],
    );

    let status_color = match app.status.as_str() {
        "failed" => Color::Red,
        "ready" | "complete" => Color::Green,
        _ => Color::Yellow,
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::Cyan)),
            Span::styled("Main", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" · "),
            Span::styled(app.status.as_str(), Style::default().fg(status_color)),
            Span::raw(format!(" · Clause {}", app.state.phase().label())),
            Span::styled(" · /q quit", Style::default().fg(Color::DarkGray)),
        ])),
        rows[3],
    );

    if rows[1].width > 2 {
        let cursor_x = rows[1]
            .x
            .saturating_add(2)
            .saturating_add(app.input.chars().count() as u16)
            .min(rows[1].right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, rows[1].y));
    }
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

    fn render_text(app: &App, width: u16, height: u16) -> String {
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
        app.input = "next question".into();
        app.transcript = vec![
            (Speaker::Operator, "FIRST".into()),
            (Speaker::North, "first answer".into()),
            (Speaker::System, "visible diagnostic".into()),
        ];
        app.status = "complete".into();
        app
    }

    #[test]
    fn headless_surface_matches_the_accepted_product_frame() {
        let rendered = render_text(&accepted_frame_app(), 110, 12);

        assert!(
            rendered.contains(
                "Agents | Goals | All > gpt-example high · /tmp/demo · north-v2-usable-tui"
            )
        );
        assert!(rendered.contains("❯ next question"));
        assert!(rendered.contains("you> FIRST"));
        assert!(rendered.contains("north> first answer"));
        assert!(rendered.contains("system> visible diagnostic"));
        assert!(rendered.contains("› Main · complete · Clause idle · /q quit"));

        assert!(!rendered.contains("North TUI"));
        assert!(!rendered.contains("Conversation"));
        assert!(!rendered.contains("Prompt (/q to quit)"));
    }

    #[test]
    fn transcript_keeps_the_newest_turn_visible() {
        let mut app = accepted_frame_app();
        app.transcript = (0..20)
            .map(|index| (Speaker::North, format!("answer {index}")))
            .collect();

        let rendered = render_text(&app, 80, 10);
        assert!(rendered.contains("north> answer 19"));
        assert!(!rendered.contains("north> answer 0 "));
    }
}
