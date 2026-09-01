mod agent_catalog;
mod clause_state;
mod codex;
mod error;

use std::env;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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
    view: View,
    state: NorthState,
    codex: Option<Codex>,
    model: String,
    reasoning_effort: String,
    input: String,
    transcript: Vec<(Speaker, String)>,
    status: String,
    turn: Option<JoinHandle<TurnCompletion>>,
    interrupt: Option<oneshot::Sender<()>>,
    turn_started_at: Option<Instant>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Agents,
    Goals,
    All,
}

impl View {
    fn next(self) -> Self {
        match self {
            Self::Agents => Self::Goals,
            Self::Goals => Self::All,
            Self::All => Self::Agents,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Agents => Self::All,
            Self::Goals => Self::Agents,
            Self::All => Self::Goals,
        }
    }
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
            view: View::Agents,
            state,
            codex: None,
            model: "Codex default".into(),
            reasoning_effort: "default".into(),
            input: String::new(),
            transcript: Vec::new(),
            status: "idle".into(),
            turn: None,
            interrupt: None,
            turn_started_at: None,
        })
    }

    fn submit(&mut self, prompt: String) {
        self.transcript.push((Speaker::Operator, prompt.clone()));
        match delegation_task(&prompt) {
            Ok(Some(task)) => self.submit_delegation(task.to_owned()),
            Ok(None) => self.submit_direct(prompt),
            Err(error) => self.record_error(error),
        }
    }

    fn submit_direct(&mut self, prompt: String) {
        if let Err(error) = self.state.submit() {
            self.record_error(error);
            return;
        }
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
            let result = codex.run_turn_interruptible(&prompt, interrupt_rx).await;
            TurnCompletion {
                codex,
                result: TurnResult::Direct(result),
            }
        }));
    }

    fn submit_delegation(&mut self, task: String) {
        if let Err(error) = self.state.delegate() {
            self.record_error(error);
            return;
        }
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
                    &task,
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
    app.status = "connecting".into();
    draw(&mut terminal, &app)?;
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
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        if key.code == KeyCode::Esc && app.is_working() {
            app.interrupt_turn();
            continue;
        }
        if navigate_view(&mut app.view, &key.code) {
            continue;
        }
        match key.code {
            KeyCode::Char(character) => app.input.push(character),
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Enter => {
                if app.input == "/q" {
                    break;
                }
                if app.input.is_empty() || app.is_working() {
                    continue;
                }
                let prompt = std::mem::take(&mut app.input);
                app.submit(prompt);
            }
            _ => {}
        }
    }
    Ok(())
}

fn navigate_view(view: &mut View, key: &KeyCode) -> bool {
    match key {
        KeyCode::Tab | KeyCode::Right => *view = view.next(),
        KeyCode::BackTab | KeyCode::Left => *view = view.previous(),
        KeyCode::Esc if *view != View::Agents => *view = View::Agents,
        _ => return false,
    }
    true
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
            Constraint::Length(u16::from(app.is_working())),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    match app.view {
        View::Agents => {
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
        View::Goals => frame.render_widget(Paragraph::new("No Goals"), rows[0]),
        View::All => frame.render_widget(Paragraph::new("No tracked things"), rows[0]),
    }

    if app.is_working() {
        frame.render_widget(Paragraph::new(working_line(app)), rows[1]);
    }

    let composer_style = Style::default()
        .fg(Color::Rgb(229, 231, 235))
        .bg(Color::Rgb(37, 39, 45));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Color::Cyan)),
            Span::styled(app.input.as_str(), composer_style),
        ]))
        .style(composer_style),
        rows[2],
    );

    let active_tab_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let inactive_tab_style = Style::default().fg(Color::DarkGray);
    let tab_style = |view| {
        if app.view == view {
            active_tab_style
        } else {
            inactive_tab_style
        }
    };
    let view_context = match app.view {
        View::Agents => format!(
            "{} {} · {} · {}",
            app.model,
            app.reasoning_effort,
            app.cwd.display(),
            app.branch
        ),
        View::Goals => "desired outcomes".into(),
        View::All => "all tracked things".into(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Agents", tab_style(View::Agents)),
            Span::styled(" | ", inactive_tab_style),
            Span::styled("Goals", tab_style(View::Goals)),
            Span::styled(" | ", inactive_tab_style),
            Span::styled("All", tab_style(View::All)),
            Span::styled(" > ", inactive_tab_style),
            Span::raw(view_context),
        ])),
        rows[3],
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
        " · /q quit",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(footer)), rows[4]);

    if rows[2].width > 2 {
        let cursor_x = rows[2]
            .x
            .saturating_add(2)
            .saturating_add(app.input.chars().count() as u16)
            .min(rows[2].right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, rows[2].y));
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
                let mut markdown = tui_markdown::from_str(message);
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
        assert!(rendered.contains("› FIRST"));
        assert!(rendered.contains("• first answer"));
        assert!(rendered.contains("• visible diagnostic"));
        assert!(rendered.contains("› Main · /q quit"));
        assert!(!rendered.contains("you>"));
        assert!(!rendered.contains("north>"));
        assert!(!rendered.contains("Clause"));

        assert!(!rendered.contains("North TUI"));
        assert!(!rendered.contains("Conversation"));
        assert!(!rendered.contains("Prompt (/q to quit)"));
    }

    #[test]
    fn tab_and_arrow_keys_navigate_the_three_product_views() {
        let mut view = View::Agents;

        assert!(navigate_view(&mut view, &KeyCode::Tab));
        assert_eq!(view, View::Goals);
        assert!(navigate_view(&mut view, &KeyCode::Right));
        assert_eq!(view, View::All);
        assert!(navigate_view(&mut view, &KeyCode::Tab));
        assert_eq!(view, View::Agents);
        assert!(navigate_view(&mut view, &KeyCode::Left));
        assert_eq!(view, View::All);
        assert!(navigate_view(&mut view, &KeyCode::BackTab));
        assert_eq!(view, View::Goals);
        assert!(navigate_view(&mut view, &KeyCode::Esc));
        assert_eq!(view, View::Agents);
        assert!(!navigate_view(&mut view, &KeyCode::Up));
    }

    #[test]
    fn goals_and_all_render_their_established_empty_states() {
        let mut app = accepted_frame_app();

        app.view = View::Goals;
        let goals = render_text(&app, 110, 12);
        assert!(goals.contains("Agents | Goals | All > desired outcomes"));
        assert!(goals.contains("No Goals"));
        assert!(!goals.contains("first answer"));

        app.view = View::All;
        let all = render_text(&app, 110, 12);
        assert!(all.contains("Agents | Goals | All > all tracked things"));
        assert!(all.contains("No tracked things"));
        assert!(!all.contains("first answer"));
    }

    #[test]
    fn transcript_keeps_the_newest_turn_visible() {
        let mut app = accepted_frame_app();
        app.transcript = (0..20)
            .map(|index| (Speaker::North, format!("answer {index}")))
            .collect();

        let rendered = render_text(&app, 80, 10);
        assert!(rendered.contains("• answer 19"));
        assert!(!rendered.contains("• answer 0 "));
    }

    #[test]
    fn empty_agents_view_is_a_truthful_welcome_card() {
        let mut app = App::open(PathBuf::from("/home/tom/demo")).unwrap();
        app.model = "gpt-5.6-sol".into();
        app.reasoning_effort = "low".into();

        let rendered = render_text(&app, 100, 15);

        assert!(rendered.contains("North (v0.1.0)"));
        assert!(rendered.contains("model:       gpt-5.6-sol low"));
        assert!(rendered.contains("directory:   /home/tom/demo"));
        assert!(rendered.contains("permissions: workspace write, no approval prompts"));
        assert!(rendered.contains("› Main · /q quit"));
        assert!(!rendered.contains("Main (ready)"));
        assert!(!rendered.contains("· ready ·"));
    }

    #[test]
    fn conversation_replaces_the_welcome_card_and_highlights_operator_turns() {
        let app = accepted_frame_app();
        let mut terminal = Terminal::new(TestBackend::new(100, 15)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(!render_text(&app, 100, 15).contains("North (v0.1.0)"));
        assert_eq!(buffer[(1, 1)].bg, Color::Rgb(52, 58, 62));
        assert_eq!(buffer[(40, 1)].bg, Color::Rgb(52, 58, 62));
    }

    #[test]
    fn agent_responses_render_commonmark_instead_of_showing_source_markers() {
        let mut app = accepted_frame_app();
        app.transcript = vec![(
            Speaker::North,
            concat!(
                "# Heading\n\n",
                "I’m **Codex, based on GPT-5**, with *emphasis* and `code`.\n\n",
                "- one\n- two\n\n",
                "> quote\n\n",
                "[OpenAI](https://openai.com)\n\n",
                "| A | B |\n|---|---|\n| 1 | 2 |"
            )
            .into(),
        )];

        let text = conversation_text(&app, 100);
        let rendered = text.to_string();
        assert!(rendered.contains("Heading"));
        assert!(rendered.contains("Codex, based on GPT-5"));
        assert!(rendered.contains("one"));
        assert!(rendered.contains("quote"));
        assert!(rendered.contains("OpenAI"));
        assert!(rendered.contains("https://openai.com"));
        assert!(rendered.contains('┌'));
        assert!(!rendered.contains("**"));
        assert!(!rendered.contains("`code`"));
        assert!(text.lines.iter().flat_map(|line| &line.spans).any(|span| {
            span.content.contains("Codex, based on GPT-5")
                && span.style.add_modifier.contains(Modifier::BOLD)
        }));
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

        let rendered = render_text(&app, 80, 10);
        assert!(rendered.contains("• Interrupted"));
        assert!(rendered.contains("› Main · /q quit"));
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
