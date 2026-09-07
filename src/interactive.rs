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
        let buffer = self.terminal.draw(|frame| render(frame, app))?.buffer.clone();
        let editor = (app.picker.is_none() && app.state.menu().kind.is_empty()
            && app.state.active_prompt().is_none() && app.transcript_search.is_none())
            .then(|| app.composer.textarea().clone());
        self.output.send(Output::View(View { buffer, editor, acknowledged: self.acknowledged }))
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
        if !event::poll(Duration::from_millis(8))? { continue; }
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
        .min(prior_area.height.saturating_sub(3).max(1));
    let prior_row = Rect::new(prior_area.x, prior_area.bottom().saturating_sub(2 + prior_height), prior_area.width, prior_height);
    for input in pending {
        if !preview(&mut editor, &input.event) { break; }
    }
    let area = padded(frame.area());
    let height = editor.measure(area.width.saturating_sub(2).max(1)).preferred_rows
        .min(area.height.saturating_sub(3).max(1));
    let row = Rect::new(area.x, area.bottom().saturating_sub(2 + height), area.width, height);
    let style = Style::default().fg(Color::Rgb(229, 231, 235)).bg(Color::Rgb(37, 39, 45));
    frame.render_widget(ratatui::widgets::Clear, prior_row.intersection(frame.area()));
    frame.render_widget(ratatui::widgets::Clear, row);
    frame.render_widget(Paragraph::new("").style(style), row);
    frame.render_widget(Paragraph::new(Span::styled("❯ ", style.fg(Color::Cyan))), Rect { width: row.width.min(2), ..row });
    if row.width > 2 {
        frame.render_widget(&editor, Rect { x: row.x + 2, width: row.width - 2, ..row });
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
        let y = content.bottom() - 3;
        buffer.set_string(content.x, y, "❯ qzxvkjwp", Style::default());
        let view = View { buffer, editor: Some(editor), acknowledged: 8 };
        let pending = (9..13).map(|sequence| PendingInput {
            sequence, event: Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        }).collect();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        let result = terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap();
        let line = (content.x..content.right()).map(|x| result.buffer[(x, y)].symbol()).collect::<String>();
        assert_eq!(line.trim_end(), "❯ qzxv");
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
