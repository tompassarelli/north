//! Terminal I/O stays independent of the ordered application worker. Editor
//! previews replay only unacknowledged input over an authoritative frame.
use std::collections::VecDeque;
use std::env;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal, TerminalOptions, Viewport, buffer::Buffer, layout::Rect};
use tui_textarea::TextArea;

use crate::command_surface::{Picker, menu_direction, render_slash_menu};
use crate::composer::Composer;
use crate::error::{NorthError, NorthResult};
use crate::{command_surface, composer, north_state};

use super::app::{App, Speaker, navigate_view};
use super::render::{padded, render_application};

struct PendingInput {
    sequence: u64,
    event: Event,
}

struct View {
    buffer: Buffer,
    editor: Option<TextArea<'static>>,
    acknowledged: u64,
    commands: Vec<north_state::CommandSpec>,
    slash_menu: command_surface::SlashMenu,
    reference_menu: bool,
}

enum Output {
    View(View),
    EditSource(PathBuf, Sender<NorthResult<()>>),
    Done(NorthResult<()>),
}

pub(super) enum Input {
    Event(Event),
    Idle,
    Closed,
}

pub(super) struct WorkerUi {
    terminal: Terminal<CrosstermBackend<io::Sink>>,
    input: Receiver<PendingInput>,
    output: Sender<Output>,
    acknowledged: u64,
}

impl WorkerUi {
    fn new(area: Rect, input: Receiver<PendingInput>, output: Sender<Output>) -> NorthResult<Self> {
        Ok(Self { terminal: Self::virtual_terminal(area)?, input, output, acknowledged: 0 })
    }

    fn virtual_terminal(area: Rect) -> NorthResult<Terminal<CrosstermBackend<io::Sink>>> {
        Ok(Terminal::with_options(CrosstermBackend::new(io::sink()), TerminalOptions {
            viewport: Viewport::Fixed(area),
        })?)
    }

    pub(super) fn read_event(&mut self, timeout: Duration) -> NorthResult<Input> {
        match self.input.recv_timeout(timeout) {
            Ok(input) => {
                self.acknowledged = input.sequence;
                if let Event::Resize(width, height) = input.event {
                    // Terminal::resize asks the process's real tty for its size; this terminal only draws to a sink.
                    self.terminal = Self::virtual_terminal(Rect::new(0, 0, width, height))?;
                }
                Ok(Input::Event(input.event))
            }
            Err(RecvTimeoutError::Timeout) => Ok(Input::Idle),
            Err(RecvTimeoutError::Disconnected) => Ok(Input::Closed),
        }
    }

    pub(super) fn draw(&mut self, app: &mut App) -> NorthResult<()> {
        let buffer = self.terminal.draw(|frame| render_application(frame, app, false))?.buffer.clone();
        let editor = (app.picker.is_none() && app.state.menu().kind.is_empty()
            && app.state.active_prompt().is_none() && app.transcript_search.is_none())
            .then(|| app.composer.textarea().clone());
        self.output.send(Output::View(View {
            buffer, editor, acknowledged: self.acknowledged,
            commands: app.state.commands().to_vec(), slash_menu: app.slash_menu.clone(),
            reference_menu: app.reference_query().is_some(),
        }))
            .map_err(|_| NorthError::Configuration("The terminal has closed".into()))
    }

    pub(super) fn edit_source(&mut self, path: &Path) -> NorthResult<()> {
        let (reply, response) = mpsc::channel();
        self.output.send(Output::EditSource(path.to_owned(), reply))
            .map_err(|_| NorthError::Configuration("The terminal has closed".into()))?;
        response.recv().map_err(|_| NorthError::Configuration("The terminal has closed".into()))?
    }
}

pub(crate) async fn run(cwd: PathBuf, requested: Option<String>) -> NorthResult<()> {
    let (_session, mut terminal) = TerminalSession::enter()?;
    let size = terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);
    let (input_tx, input_rx) = mpsc::channel();
    let (output_tx, output_rx) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let worker = std::thread::Builder::new().name("north-application".into()).spawn(move || {
        let result = runtime.block_on(async {
            let mut ui = WorkerUi::new(area, input_rx, output_tx.clone())?;
            let mut app = App::open(cwd)?;
            app.status = "connecting".into();
            ui.draw(&mut app)?;
            match app.ensure_codex(requested.as_deref()).await {
                Ok(()) => app.status = "idle".into(),
                Err(error) if requested.is_some() => { app.shutdown().await; return Err(error); }
                Err(error) => app.record_error(error),
            }
            let result = application_loop(&mut ui, &mut app).await;
            app.shutdown().await;
            result
        });
        let _ = output_tx.send(Output::Done(result));
    })?;
    let result = foreground(&mut terminal, &input_tx, &output_rx);
    drop(input_tx);
    worker.join().map_err(|_| NorthError::Configuration("North stopped unexpectedly".into()))?;
    terminal.show_cursor()?;
    result
}

fn foreground(terminal: &mut NorthTerminal, input: &Sender<PendingInput>, output: &Receiver<Output>) -> NorthResult<()> {
    let mut view: Option<View> = None;
    let mut pending = VecDeque::<PendingInput>::new();
    let mut sequence = 0;
    let mut redraw = true;
    loop {
        loop {
            let message = match output.try_recv() {
                Ok(message) => message,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(NorthError::Configuration("North stopped unexpectedly".into())),
            };
            match message {
                Output::View(next) => {
                    while pending.front().is_some_and(|input| input.sequence <= next.acknowledged) { pending.pop_front(); }
                    view = Some(next);
                    redraw = true;
                }
                Output::EditSource(path, reply) => {
                    let _ = reply.send(edit_source(terminal, &path));
                    redraw = true;
                }
                Output::Done(result) => return result,
            }
        }
        if redraw {
            terminal.draw(|frame| paint(frame, view.as_ref(), &pending))?;
            redraw = false;
        }
        if !event::poll(Duration::from_millis(2))? { continue; }
        let event = event::read()?;
        sequence += 1;
        input.send(PendingInput { sequence, event: event.clone() })
            .map_err(|_| NorthError::Configuration("North stopped unexpectedly".into()))?;
        pending.push_back(PendingInput { sequence, event });
        redraw = true;
    }
}

fn preview(editor: &mut TextArea<'static>, event: &Event) -> bool {
    match event {
        Event::Paste(text) => { editor.insert_str(text.replace('\r', "\n")); true }
        Event::Resize(..) => true,
        Event::Key(key) if key.kind != KeyEventKind::Press => true,
        Event::Key(key) if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) => false,
        Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !matches!(key.code, KeyCode::Char('a' | 'e' | 'u' | 'k' | 'w' | 'z')) { return false; }
            composer::edit_textarea(editor, *key);
            true
        }
        Event::Key(key) => match key.code {
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {
                composer::edit_textarea(editor, *key);
                true
            }
            KeyCode::Enter | KeyCode::Tab if !editor.is_empty() && !editor.lines()[0].starts_with('/') => {
                *editor = Composer::new().textarea().clone();
                true
            }
            _ => false,
        },
        _ => false,
    }
}

fn preview_input(
    editor: &mut TextArea<'static>,
    menu: &mut command_surface::SlashMenu,
    view: &View,
    event: &Event,
) -> bool {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press { return true; }
        if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) { return false; }
        if view.reference_menu {
            if menu_direction(key).is_some() || matches!(key.code, KeyCode::Tab | KeyCode::Enter) {
                return false;
            }
        } else {
            match command_surface::slash_action(&view.commands, &editor.lines().join("\n"), menu, key) {
                command_surface::SlashAction::Navigate | command_surface::SlashAction::Dismiss => return true,
                command_surface::SlashAction::Complete(command) => {
                    *editor = Composer::new().textarea().clone();
                    editor.insert_str(command);
                    return true;
                }
                command_surface::SlashAction::Submit(_) => return false,
                command_surface::SlashAction::Unhandled => {}
            }
        }
    }
    let handled = preview(editor, event);
    if handled && matches!(event, Event::Key(key) if !matches!(key.code, KeyCode::Enter | KeyCode::Tab)) {
        menu.selected = 0;
    }
    menu.observe_input(&editor.lines().join("\n"));
    handled
}

fn paint(frame: &mut Frame<'_>, view: Option<&View>, pending: &VecDeque<PendingInput>) {
    let Some(view) = view else {
        frame.render_widget(Paragraph::new("Opening conversation…"), padded(frame.area()));
        return;
    };
    let overlap = frame.area().intersection(view.buffer.area);
    for y in overlap.y..overlap.bottom() {
        for x in overlap.x..overlap.right() {
            frame.buffer_mut()[(x, y)] = view.buffer[(x, y)].clone();
        }
    }
    let Some(mut editor) = view.editor.clone() else { return; };
    let prior_area = padded(view.buffer.area);
    let prior_height = editor.measure(prior_area.width.saturating_sub(2).max(1)).preferred_rows
        .min(prior_area.height.saturating_sub(2).max(1));
    let prior_row = Rect::new(prior_area.x, prior_area.bottom().saturating_sub(1 + prior_height), prior_area.width, prior_height);
    let mut menu = view.slash_menu.clone();
    for input in pending {
        if !preview_input(&mut editor, &mut menu, view, &input.event) { break; }
    }
    let area = padded(frame.area());
    let height = editor.measure(area.width.saturating_sub(2).max(1)).preferred_rows
        .min(area.height.saturating_sub(2).max(1));
    let row = Rect::new(area.x, area.bottom().saturating_sub(1 + height), area.width, height);
    let style = Style::default().fg(Color::Rgb(229, 231, 235)).bg(Color::Rgb(37, 39, 45));
    frame.render_widget(ratatui::widgets::Clear, prior_row.intersection(frame.area()));
    frame.render_widget(ratatui::widgets::Clear, row);
    frame.render_widget(Paragraph::new("").style(style), row);
    frame.render_widget(Paragraph::new(Span::styled("❯ ", style.fg(Color::Cyan))), Rect { width: row.width.min(2), ..row });
    if row.width > 2 {
        frame.render_widget(&editor, Rect { x: row.x + 2, width: row.width - 2, ..row });
    }
    if !view.reference_menu && menu.visible() {
        render_slash_menu(frame, row, &view.commands, &editor.lines().join("\n"), menu.selected);
    }
}


pub(super) async fn application_loop(terminal: &mut WorkerUi, app: &mut App) -> NorthResult<()> {
    loop {
        app.refresh_reference_menu();
        draw(terminal, app)?;
        let input = match terminal.read_event(Duration::ZERO)? {
            Input::Idle => {
                let events_pending = app.collect_events(Some(Duration::from_millis(8)));
                app.collect_usage().await;
                app.collect_prompt_responses().await;
                app.collect_steering().await;
                app.collect_finished_turn().await;
                if let Err(error) = app.collect_reconnect().await { app.record_error(error); }
                if let Err(error) = app.dispatch_ready_work() { app.record_error(error); }
                app.refresh_reference_menu();
                draw(terminal, app)?;
                terminal.read_event(if events_pending { Duration::ZERO } else { Duration::from_millis(50) })?
            }
            input => input,
        };
        let terminal_event = match input {
            Input::Event(event) => event,
            Input::Idle => { app.save_composer()?; continue; }
            Input::Closed => break,
        };
        if let Event::Paste(pasted) = terminal_event {
            app.sync_prompt_editor();
            if !app.state.menu().kind.is_empty() {
                app.menu_editor.insert_str(pasted.replace('\r', "\n"));
                app.state.query_menu(&app.menu_editor.lines().join("\n"))?;
            } else if let Some(editor) = app.transcript_search.as_mut() {
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
        if app.handle_menu_key(key).await? { continue; }
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
                        && let Err(error) = terminal.edit_source(&unit.source)
                    {
                        app.record_error(error);
                    }
                }
                KeyCode::Enter => app.accept_picker_selection().await,
                _ => {}
            }
            continue;
        }
        if app.handle_transcript_key(key)? { continue; }
        if app.handle_reference_key(&key) {
            continue;
        }
        if key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::ALT) {
            if let Err(error) = app.edit_pending_input() { app.record_error(error); }
            continue;
        }
        if key.code == KeyCode::Esc && app.slash_menu.dismiss(app.state.commands(), &app.composer.text()) {
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
                Ok(file) => match app.state.retain_image_file(file.path()).and_then(|()| app.state.attach_image()) {
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
        match command_surface::slash_action(app.state.commands(), &app.composer.text(), &mut app.slash_menu, &key) {
            command_surface::SlashAction::Navigate | command_surface::SlashAction::Dismiss => continue,
            command_surface::SlashAction::Complete(command) => {
                let removed = app.composer.replace_text(command);
                app.detach_images(removed);
                continue;
            }
            command_surface::SlashAction::Submit(command) => {
                let removed = app.composer.replace_text(command);
                app.detach_images(removed);
                let submission = app.composer.take_submission();
                if app.accept_submission(submission).await { break; }
                app.slash_menu.selected = 0;
                continue;
            }
            command_surface::SlashAction::Unhandled => {}
        }
        if app.handle_composer_key(key).await { break; }
    }
    app.save_composer_now()?;
    Ok(())
}

pub(super) fn draw(terminal: &mut WorkerUi, app: &mut App) -> NorthResult<()> {
    terminal.draw(app)
}

type NorthTerminal = Terminal<CrosstermBackend<Stdout>>;

pub(super) struct TerminalSession;

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

pub(super) fn editor_command(editor: &str, source: &Path) -> Command {
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

#[cfg(test)]
mod tests;
