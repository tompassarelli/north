use super::*;
use crate::north_state::NorthState;
use crossterm::event::KeyEvent;

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
    app.composer.replace_text("/model");
    ui.acknowledged = 7;
    ui.draw(&mut app).unwrap();
    let Output::View(next) = frames.recv().unwrap() else { panic!("expected frame"); };
    view = next;
    pending = (8..12).map(|sequence| PendingInput {
        sequence, event: Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
    }).collect();
    let shortened = buffer_text(terminal.draw(|frame| paint(frame, Some(&view), &pending)).unwrap().buffer);
    assert!(shortened.contains("❯ /m"), "{shortened}");
    assert!(!shortened.contains("❯ /model"), "{shortened}");
    pending.push_back(PendingInput { sequence: 12, event: Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)) });
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
    application_loop(&mut ui, &mut app).await.unwrap();
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
    application_loop(&mut ui, &mut app).await.unwrap();
    assert_eq!(app.composer.text(), "qz");
    drop(app);
    let reopened = App::open_stored(cwd, &store).unwrap();
    assert_eq!(reopened.composer.text(), "qz");
    assert!(reopened.state.pending_inputs().is_empty());
}
