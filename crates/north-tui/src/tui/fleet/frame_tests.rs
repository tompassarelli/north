use std::path::PathBuf;

use crate::fleet::model::{ProviderName, SessionRecord, Status, Account};

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::desk::SessionSummary;
use super::help_panel::Tab;
use super::painter::TranscriptPainter;
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
    SessionSummary { record, status, last_activity_ms: Some(created_ms), since_ms, depth: 0 }
}

fn names(screen: &Screen) -> Vec<&str> {
    screen.sessions.iter().map(|session| session.record.name.as_str()).collect()
}

fn account() -> Account {
    Account { provider: ProviderName::new("claude"), label: "work".into(), home: PathBuf::from("/accounts/work"), token_file: None, api: None }
}

#[test]
fn rows_keep_their_places_as_output_arrives_from_different_sessions() {
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
fn help_with_the_proxy_focused_shows_the_proxys_account_and_log_path() {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.sync(vec![lodged("lander-precheck", "c", 1, Status::Idle), lodged("proxy", "e", 2, Status::Parked)]);
    screen.help.open(Tab::General);
    assert!(help_fact(&mut screen, "Account:").ends_with("e (claude)"));
    assert!(help_fact(&mut screen, "Session Logs:").ends_with("/logs/proxy"));
}
