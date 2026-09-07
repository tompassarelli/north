//! Terminal I/O stays independent of the ordered application worker. Editor
//! previews replay only unacknowledged input over an authoritative frame.
use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, RecvTimeoutError, TryRecvError};
use ratatui::{buffer::Buffer, TerminalOptions, Viewport};
use tui_textarea::TextArea;

struct PendingInput {
    sequence: u64,
    event: Event,
}

struct View {
    buffer: Buffer,
    editor: Option<TextArea<'static>>,
    acknowledged: u64,
    commands: Vec<clause_state::CommandSpec>,
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
        Ok(Self {
            terminal: Terminal::with_options(CrosstermBackend::new(io::sink()), TerminalOptions {
                viewport: Viewport::Fixed(area),
            })?,
            input, output, acknowledged: 0,
        })
    }

    pub(super) fn read_event(&mut self, timeout: Duration) -> NorthResult<Input> {
        match self.input.recv_timeout(timeout) {
            Ok(input) => {
                self.acknowledged = input.sequence;
                if let Event::Resize(width, height) = input.event {
                    self.terminal.resize(Rect::new(0, 0, width, height))?;
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

pub(super) async fn run(cwd: PathBuf, requested: Option<String>) -> NorthResult<()> {
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
            let result = super::run(&mut ui, &mut app).await;
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
                    let _ = reply.send(super::edit_source(terminal, &path));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unacknowledged_edits_replay_over_a_new_frame_without_duplication() {
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let edits = [key(KeyCode::Char('q')), key(KeyCode::Char('z')), key(KeyCode::Backspace), key(KeyCode::Char('c'))];
        let mut editor = Composer::new().textarea().clone();
        for event in &edits { assert!(preview(&mut editor, event)); }
        assert_eq!(editor.lines().join("\n"), "qc");
        let mut acknowledged = Composer::new().textarea().clone();
        acknowledged.insert_str("q");
        for event in &edits[1..] { assert!(preview(&mut acknowledged, event)); }
        assert_eq!(acknowledged.lines(), editor.lines());
        assert!(preview(&mut acknowledged, &key(KeyCode::Enter)));
        assert!(preview(&mut acknowledged, &key(KeyCode::Char('n'))));
        assert_eq!(acknowledged.lines().join("\n"), "n");
        assert!(!preview(&mut acknowledged, &Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL))));
        assert_eq!(acknowledged.lines().join("\n"), "n");
        let mut command = Composer::new().textarea().clone();
        command.insert_str("/help");
        assert!(!preview(&mut command, &key(KeyCode::Enter)));
    }

    #[test]
    fn deleting_pending_text_clears_the_longer_acknowledged_frame() {
        let area = Rect::new(0, 0, 100, 40);
        let mut editor = Composer::new().textarea().clone();
        editor.insert_str("qzxvkjwp");
        let mut buffer = Buffer::empty(area);
        let content = padded(area);
        let y = content.bottom() - 2;
        buffer.set_string(content.x, y, "❯ qzxvkjwp", Style::default());
        let view = View { buffer, editor: Some(editor), acknowledged: 8, commands: Vec::new(), slash_menu: command_surface::SlashMenu::default(), reference_menu: false };
        let pending = (9..13).map(|sequence| PendingInput {
            sequence, event: Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        }).collect();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let result = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap();
        let line = (content.x..content.right()).map(|x| result.buffer[(x, y)].symbol()).collect::<String>();
        assert_eq!(line.trim_end(), "❯ qzxv");
    }

    fn buffer_text(buffer: &Buffer) -> String {
        (buffer.area.y..buffer.area.bottom()).map(|y| {
            (buffer.area.x..buffer.area.right()).map(|x| buffer[(x, y)].symbol()).collect::<String>()
        }).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn busy_worker_slash_menu_filters_navigates_completes_and_clears_across_frames() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let mut app = App::open_stored(cwd, &root.path().join("state")).unwrap();
        let (_input, receive) = mpsc::channel();
        let (output, frames) = mpsc::channel();
        let mut ui = WorkerUi::new(Rect::new(0, 0, 100, 40), receive, output).unwrap();
        ui.draw(&mut app).unwrap();
        let Output::View(mut view) = frames.recv().unwrap() else { panic!("expected frame"); };
        let first = view.commands[0].name().to_owned();
        let second = view.commands[1].name().to_owned();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let mut pending = VecDeque::new();
        pending.push_back(PendingInput { sequence: 1, event: Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)) });
        let rendered = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&rendered).contains(view.commands[0].description()));
        assert!(buffer_text(&rendered).contains(view.commands[1].description()));
        for (sequence, code, modifiers, expected) in [
            (2, KeyCode::Down, KeyModifiers::NONE, second.as_str()),
            (3, KeyCode::Char('k'), KeyModifiers::CONTROL, first.as_str()),
            (4, KeyCode::Char('j'), KeyModifiers::CONTROL, second.as_str()),
            (5, KeyCode::Up, KeyModifiers::NONE, first.as_str()),
        ] {
            pending.push_back(PendingInput { sequence, event: Event::Key(KeyEvent::new(code, modifiers)) });
            let rendered = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
            let selected = rendered.content.iter().filter(|cell| cell.fg == Color::Green).map(|cell| cell.symbol()).collect::<String>();
            assert!(selected.contains(expected), "expected {expected} in {selected}");
        }
        pending.push_back(PendingInput { sequence: 6, event: Event::Key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)) });
        let filtered = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&filtered).contains("/model"));
        assert!(!buffer_text(&filtered).contains(view.commands[0].description()));
        pending.push_back(PendingInput { sequence: 7, event: Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)) });
        let completed = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&completed).contains("❯ /model"));

        // The worker catches up only through the slash. Its base frame must not
        // retain an overlay that can outlive the newer foreground edits.
        app.composer.insert_text("/");
        ui.acknowledged = 1;
        ui.draw(&mut app).unwrap();
        let Output::View(next) = frames.recv().unwrap() else { panic!("expected frame"); };
        assert!(!buffer_text(&next.buffer).contains(view.commands[0].description()));
        pending.retain(|input| input.sequence > next.acknowledged);
        view = next;
        assert_eq!(terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer, &completed);
        pending.push_back(PendingInput { sequence: 8, event: Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)) });
        assert!(buffer_text(terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer).contains("❯ /mode"));
        pending.push_back(PendingInput { sequence: 9, event: Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)) });
        let cleared = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
        assert!(!buffer_text(&cleared).contains("/model"));
        assert!(!buffer_text(&cleared).contains(view.commands[0].description()));
    }

    #[test]
    fn slash_submit_and_reference_selection_wait_for_the_worker() {
        let state = NorthState::open().unwrap();
        let mut editor = Composer::new().textarea().clone();
        editor.insert_str("/m");
        let mut view = View {
            buffer: Buffer::empty(Rect::new(0, 0, 100, 40)), editor: None, acknowledged: 0,
            commands: state.commands().to_vec(), slash_menu: command_surface::SlashMenu::default(), reference_menu: false,
        };
        let mut selected = command_surface::SlashMenu::default();
        editor = Composer::new().textarea().clone();
        editor.insert_str("/");
        assert!(preview_input(&mut editor, &mut selected, &view, &Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))));
        assert_eq!(selected.selected, 1);
        assert!(!preview_input(&mut editor, &mut selected, &view, &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))));
        assert_eq!(selected.selected, 1);
        assert_eq!(editor.lines(), &["/"]);
        assert!(preview_input(&mut editor, &mut selected, &view, &Event::Paste(String::new())));
        assert_eq!(selected.selected, 1);
        assert!(preview_input(&mut editor, &mut selected, &view, &Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))));
        assert_eq!(selected.selected, 0);
        editor.insert_str("m");
        assert!(!preview_input(&mut editor, &mut selected, &view, &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))));
        assert_eq!(editor.lines(), &["/m"]);
        view.reference_menu = true;
        for (code, modifiers) in [(KeyCode::Tab, KeyModifiers::NONE), (KeyCode::Char('k'), KeyModifiers::CONTROL)] {
            assert!(!preview_input(&mut editor, &mut selected, &view, &Event::Key(KeyEvent::new(code, modifiers))));
            assert_eq!(editor.lines(), &["/m"]);
        }
    }

    #[tokio::test]
    async fn escape_dismisses_slash_menu_without_erasing_text_across_worker_frames() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let mut app = App::open_stored(cwd, &root.path().join("state")).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("escape-thread").unwrap();
        let (input, receive) = mpsc::channel();
        let (output, frames) = mpsc::channel();
        let mut ui = WorkerUi::new(Rect::new(0, 0, 100, 40), receive, output).unwrap();
        ui.draw(&mut app).unwrap();
        let Output::View(view) = frames.recv().unwrap() else { panic!("expected frame"); };
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut pending = VecDeque::from([
            PendingInput { sequence: 1, event: key(KeyCode::Char('/')) },
            PendingInput { sequence: 2, event: key(KeyCode::Esc) },
        ]);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let dismissed = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&dismissed).contains("❯ /"));
        assert!(!buffer_text(&dismissed).contains(view.commands[0].description()));
        for event in [key(KeyCode::Char('/')), key(KeyCode::Esc), Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))].into_iter().enumerate() {
            input.send(PendingInput { sequence: event.0 as u64 + 1, event: event.1 }).unwrap();
        }
        super::super::run(&mut ui, &mut app).await.unwrap();
        assert_eq!(app.composer.text(), "/");
        assert!(!app.slash_menu.visible());
        let next = frames.try_iter().filter_map(|frame| match frame { Output::View(view) => Some(view), _ => None }).last().unwrap();
        pending.retain(|input| input.sequence > next.acknowledged);
        let caught_up = terminal.draw(|frame| paint(frame, Some(&next), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&caught_up).contains("❯ /"));
        assert!(!buffer_text(&caught_up).contains(view.commands[0].description()));
        pending.push_back(PendingInput { sequence: 4, event: key(KeyCode::Char('m')) });
        let reopened = terminal.draw(|frame| paint(frame, Some(&next), &pending)).unwrap().buffer.clone();
        assert!(buffer_text(&reopened).contains("❯ /m"));
        assert!(buffer_text(&reopened).contains("/model"));
    }

    #[tokio::test]
    async fn queued_edits_are_applied_once_and_saved_before_close() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let store = root.path().join("state");
        let mut app = App::open_stored(cwd.clone(), &store).unwrap();
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation("editor-thread").unwrap();
        let (input, receive) = mpsc::channel();
        let (output, _frames) = mpsc::channel();
        let mut ui = WorkerUi::new(Rect::new(0, 0, 100, 40), receive, output).unwrap();
        let events = [
            Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Event::Paste("zxv".into()),
            Event::Resize(90, 35),
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        ];
        for (index, event) in events.into_iter().enumerate() {
            input.send(PendingInput { sequence: index as u64 + 1, event }).unwrap();
        }
        super::super::run(&mut ui, &mut app).await.unwrap();
        assert_eq!(app.composer.text(), "qz");
        drop(app);
        let reopened = App::open_stored(cwd, &store).unwrap();
        assert_eq!(reopened.composer.text(), "qz");
        assert!(reopened.state.pending_inputs().is_empty());
    }
}
