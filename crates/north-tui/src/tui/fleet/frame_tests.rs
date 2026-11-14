use std::path::PathBuf;

use crate::fleet::model::{Account, Entry, EntryKind, ProviderName, Report, SessionPhase, SessionRecord, Status};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::desk::SessionSummary;
use super::help_panel::Tab;
use super::painter::{TranscriptPainter, paint};
use super::screen::Screen;
use super::view::draw;

fn family(name: &str, parent: Option<&str>, created_ms: u64, status: Status, since_ms: u64) -> SessionSummary {
    let record: SessionRecord = serde_json::from_value(serde_json::json!({
        "name": name,
        "provider": "claude",
        "account": "work",
        "cwd": format!("/code/{name}"),
        "model": null,
        "created_ms": created_ms,
        "host_pid": null,
        "conversation": null,
        "parent": parent,
    }))
    .unwrap();
    SessionSummary { record, status, phase: SessionPhase::Live(status), last_activity_ms: Some(created_ms), since_ms, depth: 0 }
}

fn names(screen: &Screen) -> Vec<&str> {
    screen.sessions.iter().map(|session| session.record.name.as_str()).collect()
}

fn account() -> Account {
    Account { provider: ProviderName::new("claude"), label: "work".into(), home: PathBuf::from("/accounts/work"), token_file: None, api: None }
}

#[test]
fn main_view_has_no_sessions_pane_and_no_glance_banner() {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    let start = |statuses: [Status; 6], activity: [u64; 6]| {
        let mut sessions = vec![
            family("proxy", None, 1, statuses[0], 0),
            family("kid-b", Some("proxy"), 4, statuses[1], 0),
            family("kid-a", Some("proxy"), 2, statuses[2], 0),
            family("grand", Some("kid-a"), 3, statuses[3], 0),
            family("solo", None, 5, statuses[4], 0),
            family("gone", Some("proxy"), 0, statuses[5], 0),
        ];
        for (session, activity) in sessions.iter_mut().zip(activity) {
            session.last_activity_ms = Some(activity);
        }
        sessions
    };
    use Status::{Exited, Idle, Working};
    screen.sync(start([Idle, Idle, Working, Idle, Idle, Exited], [1, 2, 3, 4, 5, 6]));
    let settled = ["proxy", "kid-a", "grand", "kid-b", "solo", "gone"];
    assert_eq!(names(&screen), settled);
    assert_eq!(screen.sessions.iter().map(|session| session.depth).collect::<Vec<_>>(), [0, 1, 2, 1, 0, 0]);
    let bursts = [
        ([Idle, Working, Working, Idle, Idle, Exited], [10, 9, 8, 7, 6, 5]),
        ([Working, Idle, Idle, Working, Working, Exited], [1, 50, 2, 90, 3, 4]),
        ([Idle, Idle, Working, Working, Idle, Exited], [99, 98, 97, 96, 95, 94]),
    ];
    for (statuses, activity) in bursts {
        screen.sync(start(statuses, activity));
        assert_eq!(names(&screen), settled, "output moved a row");
    }
    screen.sync(start([Idle, Idle, Idle, Idle, Idle, Idle], [1; 6]));
    assert_eq!(names(&screen), ["proxy", "gone", "kid-a", "grand", "kid-b", "solo"], "a session leaving the exited state may move");
    let mut asking = start([Idle; 6], [1; 6]);
    asking[1].phase = SessionPhase::Reported(Report::Blocked);
    asking[3].phase = SessionPhase::Waiting;
    asking[4].phase = SessionPhase::Reported(Report::Done);
    screen.sync(asking);
    assert_eq!(names(&screen), ["grand", "kid-b", "proxy", "gone", "kid-a", "solo"], "a session waiting or reporting other than Done goes first");
    let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
    let mut painter = TranscriptPainter::default();
    terminal.draw(|frame| draw(frame, &mut screen, None, &mut painter, 0)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = buffer.content().chunks(200).map(|row| row.iter().map(|cell| cell.symbol()).collect()).collect();
    let text = rows.join("\n");
    assert!(!text.contains("sessions") && !text.contains("glance:"));
    assert!(rows[0].starts_with("╭") && rows[0].trim_end().ends_with("╮") && rows[0].trim_end().chars().count() == 200);
}


fn lodged(name: &str, account: &str, created_ms: u64, status: Status) -> SessionSummary {
    let mut session = family(name, None, created_ms, status, 0);
    session.record.account = account.into();
    session.record.host_dir = PathBuf::from(format!("/logs/{name}"));
    session
}

fn help_fact(screen: &mut Screen, label: &str) -> String {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut painter = TranscriptPainter::default();
    terminal.draw(|frame| draw(frame, screen, None, &mut painter, 0)).unwrap();
    let buffer = terminal.backend().buffer();
    let width = usize::from(buffer.area.width);
    buffer.content().chunks(width).map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>()).find(|row| row.starts_with(label)).unwrap_or_default().trim_end().to_owned()
}

#[test]
fn help_shows_the_proxys_account_and_log_path_and_closes_when_its_slash_is_backspaced() {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.sync(vec![lodged("lander-precheck", "c", 1, Status::Idle), lodged("proxy", "e", 2, Status::Parked)]);
    screen.help.open(Tab::General);
    assert!(help_fact(&mut screen, "Account:").ends_with("e (claude)"));
    assert!(help_fact(&mut screen, "Session Logs:").ends_with("/logs/proxy"));
    screen.help.close();
    screen.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(help_fact(&mut screen, "Available Commands"), "Available Commands");
    screen.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(screen.composer.is_empty());
    assert_eq!(screen.help.tab(), None);
    assert_eq!(help_fact(&mut screen, "Available Commands"), "");
}

#[test]
fn a_reply_with_tabs_keeps_its_columns_apart() {
    let entries = [Entry { kind: EntryKind::Assistant, text: "name\tstatus\tage".into() }];
    let lines = paint(&entries, &[], 80);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 1));
    buffer.set_line(0, 0, &lines[0], 80);
    let row: String = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>().trim_end().to_owned();
    assert_eq!(row, "• name    status  age");
}

fn frame_rows(screen: &mut Screen, width: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    let mut painter = TranscriptPainter::default();
    terminal.draw(|frame| draw(frame, screen, None, &mut painter, 0)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer.content().chunks(usize::from(width)).map(|row| row.iter().map(|cell| cell.symbol()).collect()).collect()
}

#[test]
fn the_main_view_names_each_session_and_the_focused_pane() {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    let empty = frame_rows(&mut screen, 80).join("\n");
    assert!(empty.contains("Ctrl+N starts one"), "{empty}");
    screen.sync(vec![family("alpha", None, 1, Status::Working, 0), family("beta", None, 2, Status::Idle, 0)]);
    screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let strip = |screen: &mut Screen| frame_rows(screen, 80).into_iter().find(|row| row.contains("alpha")).unwrap_or_default();
    let row = strip(&mut screen);
    assert!(row.contains("alpha") && row.contains("working") && row.contains("▸beta") && row.contains("idle"), "{row}");
    assert!(!row.contains("▸alpha"), "{row}");
    assert!(row.contains("keys: composer"), "{row}");
    screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(strip(&mut screen).contains("keys: session list"));
    screen.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    screen.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert!(strip(&mut screen).contains("keys: session list"), "closing the digest returns focus to where it was");
}

#[test]
fn the_digest_scrolls_to_its_last_row_and_shows_the_five_hour_window() {
    use crate::fleet::board::{Board, Panel};
    let mut accounts = Panel::new("Accounts", "every account lasts to its reset (3 accounts)");
    accounts.lines = ["a", "b", "c"].iter().enumerate().map(|(index, name)| format!("{name}  Max 5x  5h left {}%  Thu 9:00 AM  week left", 90 - index)).collect();
    let mut infra = Panel::new("Infra", "3 machines");
    infra.lines = (0..12).map(|index| format!("machine {index} idle")).collect();
    let mut health = Panel::new("Health", "all bars ready");
    health.lines = vec!["the final health row".to_owned()];
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.digest.board = Board { first_line: "1 landings".to_owned(), panels: vec![Panel::new("Work", "1 working"), accounts, infra, health] };
    screen.digest.shown = true;
    let before = frame_rows(&mut screen, 80).join("\n");
    assert!(!before.contains("the final health row"), "{before}");
    assert_eq!(before.matches("5h left").count(), 3, "{before}");
    for _ in 0..8 {
        screen.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    }
    let after = frame_rows(&mut screen, 80).join("\n");
    assert!(after.contains("the final health row"), "{after}");
    for _ in 0..8 {
        screen.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    }
    assert_eq!(frame_rows(&mut screen, 80).join("\n"), before);
}
