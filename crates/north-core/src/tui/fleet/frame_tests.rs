use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::fleet::books::{Digest, SessionSpend};
use crate::fleet::model::{Entry, EntryKind, ProviderName, SessionRecord, Status, Transcript, Account};
use crate::fleet::turns::{Tokens, TurnSignal};

use super::desk::SessionSummary;
use super::digest::{DigestPane, SessionFacts};
use super::digest_view::digest_lines;
use super::painter::TranscriptPainter;
use super::screen::Screen;
use super::view;

const MINUTE: u64 = 60_000;

#[test]
fn slash_suggestions_filter_close_and_insert_without_sending() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut screen = table_screen();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    screen.handle_key(key(KeyCode::Char('/')));
    let rows = frame(&mut screen, None, 100, 0).join("\n");
    assert!(rows.contains("commands") && rows.contains("/config") && rows.contains("/model"), "{rows}");
    screen.handle_key(key(KeyCode::Char('m')));
    let rows = frame(&mut screen, None, 100, 0).join("\n");
    assert!(rows.contains("/model") && !rows.contains("/config"), "{rows}");
    screen.handle_key(key(KeyCode::Esc));
    assert!(!frame(&mut screen, None, 100, 0).join("\n").contains("commands"));
    screen.handle_key(key(KeyCode::Backspace));
    screen.handle_key(key(KeyCode::Down));
    assert!(screen.handle_key(key(KeyCode::Tab)).is_none());
    assert_eq!(screen.composer.lines(), &["/effort "]);
}

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
    Account { provider: ProviderName::new("claude"), label: "work".into(), home: PathBuf::from("/accounts/work"), token_file: None }
}

fn frame(screen: &mut Screen, transcript: Option<&Transcript>, width: u16, now_ms: u64) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
    terminal.draw(|frame| view::draw(frame, screen, transcript, &mut TranscriptPainter::default(), now_ms)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer.content.chunks(usize::from(buffer.area.width)).map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>().trim_end().to_owned()).collect()
}

fn table_transcript() -> Transcript {
    let table = "Here is the plan:\n\n| Step | Owner | Status | Notes |\n|------|:-----:|-------:|-------|\n| parser | alice | done | handles nested quotes and escaped pipes in every cell |\n| renderer | bob | in progress | wraps long cells at narrow widths without breaking lines |\n\nThat is all.";
    let mut transcript = Transcript::new();
    transcript.entries = vec![Entry { kind: EntryKind::Assistant, text: table.into() }];
    transcript
}

fn table_screen() -> Screen {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.sync(vec![family("boss", None, 1, Status::Working, 0)]);
    screen
}

#[test]
fn a_markdown_table_renders_as_aligned_columns_at_wide_and_narrow_panes() {
    let transcript = table_transcript();
    let wide = frame(&mut table_screen(), Some(&transcript), 160, 0);
    let header = wide.iter().find(|row| row.contains("Step")).expect("header row");
    let rule = wide.iter().find(|row| row.contains('┼')).expect("rule row");
    let gutters = |row: &str, mark: char| -> Vec<usize> { row.chars().enumerate().filter(|(_, c)| *c == mark).map(|(index, _)| index).collect() };
    let columns = gutters(header, '│');
    assert_eq!(columns.len(), 4 + 3, "three gutters between four columns inside the pane borders: {header:?}");
    assert_eq!(gutters(rule, '┼'), columns[columns.len() - 4..columns.len() - 1]);
    for cell in ["parser", "renderer"] {
        let row = wide.iter().find(|row| row.contains(cell)).expect("data row");
        assert_eq!(gutters(row, '│'), columns, "columns misaligned: {row:?}");
    }
    assert!(wide.iter().any(|row| row.contains("parser") && row.contains("alice") && row.contains("done") && row.contains("handles nested quotes and escaped pipes in every cell")), "{wide:#?}");
    assert!(!wide.iter().any(|row| row.contains("|--") || row.contains("| Step")), "raw markdown leaked: {wide:#?}");
    let narrow = frame(&mut table_screen(), Some(&transcript), 80, 0);
    assert!(narrow.iter().all(|row| row.chars().count() <= 80), "{narrow:#?}");
    assert!(!narrow.iter().any(|row| row.contains("|--") || row.contains("| Step")), "raw markdown leaked: {narrow:#?}");
    let height = |rows: &[String]| rows.iter().position(|row| row.contains("That is all")).unwrap() - rows.iter().position(|row| row.contains("Step")).unwrap();
    assert!(height(&narrow) > height(&wide), "narrow cells must wrap onto extra lines: {narrow:#?}");
    let words: Vec<&str> = narrow.iter().flat_map(|row| row.split_whitespace()).collect();
    for word in ["nested", "escaped", "narrow", "widths", "lines"] {
        assert!(words.contains(&word), "{word} lost from narrow table: {narrow:#?}");
    }
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

#[test]
fn the_list_shows_one_elapsed_time_per_session_and_the_digest_the_state_times() {
    let now = 1_000 * MINUTE;
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.sync(vec![
        family("proxy", None, now - 100 * MINUTE, Status::Idle, now - 12 * MINUTE),
        family("worker", Some("proxy"), now - 42 * MINUTE, Status::Working, now - 4 * MINUTE),
    ]);
    let rows = frame(&mut screen, None, 160, now);
    let proxy = rows.iter().find(|row| row.contains("proxy idle · 1h40m")).unwrap_or_else(|| panic!("{rows:#?}"));
    let worker = rows.iter().find(|row| row.contains("worker working · 42m")).unwrap_or_else(|| panic!("{rows:#?}"));
    assert!(worker.find("worker").unwrap() > proxy.find("proxy").unwrap(), "child must be indented under its parent: {rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("working 4m") || row.contains(" now")), "the list row stays short: {rows:#?}");
    let lines: Vec<String> = digest_lines(&DigestPane::default(), &screen.sessions, now, 160).iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect()).collect();
    assert!(lines.iter().any(|line| line.contains("proxy idle 12m · up 1h40m · started ")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.contains("worker working 4m · up 42m · started ")), "{lines:#?}");
}

#[test]
fn the_digest_follows_the_list_order_with_children_indented() {
    let mut screen = Screen::new(PathBuf::from("/code"), vec![account()]);
    screen.sync(vec![
        family("zeta", Some("alpha"), 3, Status::Working, 0),
        family("old", None, 0, Status::Exited, 0),
        family("alpha", None, 2, Status::Idle, 0),
        family("beta", None, 4, Status::Idle, 0),
    ]);
    let lines: Vec<String> = digest_lines(&DigestPane::default(), &screen.sessions, 0, 120).iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect()).collect();
    let at = |needle: &str| lines.iter().position(|line| line.trim_start().starts_with(needle)).unwrap_or_else(|| panic!("{needle} missing from {lines:#?}"));
    assert!(at("alpha") < at("zeta") && at("zeta") < at("beta") && at("beta") < at("old"), "{lines:#?}");
    let indent = |needle: &str| lines[at(needle)].len() - lines[at(needle)].trim_start().len();
    assert!(indent("zeta") > indent("alpha"));
}

#[test]
fn the_task_line_shows_the_brief_title_not_the_launch_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let brief = directory.path().join("brief-fleet-tui.md");
    std::fs::write(&brief, "\n# Box: fleet TUI transcript tables\nsecond line\n").unwrap();
    let record = family("tui", None, 1, Status::Working, 0).record;
    let prompt = format!("Read and do the box brief at {}", brief.display());
    let mut transcript = Transcript::new();
    transcript.entries = vec![Entry { kind: EntryKind::User, text: prompt.clone() }];
    assert_eq!(SessionFacts::of(&record, Some(&transcript)).task.as_deref(), Some("Box: fleet TUI transcript tables"));
    transcript.entries = vec![Entry { kind: EntryKind::User, text: "Read and do the box brief at /nonexistent/brief.md".into() }];
    assert_eq!(SessionFacts::of(&record, Some(&transcript)).task.as_deref(), Some("Read and do the box brief at /nonexistent/brief.md"));
    let mut tagged = record.clone();
    tagged.task_ref = Some("T-42".into());
    assert_eq!(SessionFacts::of(&tagged, Some(&transcript)).task.as_deref(), Some("T-42"));
}

#[test]
fn the_task_line_skips_a_first_task_notification_for_the_brief_title_or_task_ref() {
    let directory = tempfile::tempdir().unwrap();
    let brief = directory.path().join("brief-tui-unify.md");
    std::fs::write(&brief, "# Box: one TUI\n").unwrap();
    let record = family("tui", None, 1, Status::Working, 0).record;
    let mut transcript = Transcript::new();
    transcript.entries = vec![
        Entry { kind: EntryKind::User, text: "<task-notification>child started</task-notification>".into() },
        Entry { kind: EntryKind::User, text: format!("Read the brief at {}", brief.display()) },
    ];
    let facts = SessionFacts::of(&record, Some(&transcript));
    assert_eq!(facts.task.as_deref(), Some("Box: one TUI"));
    let summary = family("tui", None, 1, Status::Working, 0);
    let mut digest = DigestPane::default();
    digest.facts.insert("tui".into(), facts);
    let rendered = digest_lines(&digest, &[summary], 0, 160).iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(rendered.contains("Box: one TUI"), "{rendered}");
    assert!(!rendered.contains("<task-notification>"), "{rendered}");
    let mut tagged = record;
    tagged.task_ref = Some("north#418".into());
    transcript.entries.truncate(1);
    assert_eq!(SessionFacts::of(&tagged, Some(&transcript)).task.as_deref(), Some("north#418"));
}

#[test]
fn a_working_session_shows_live_tokens_from_usage_frames_before_its_turn_ends() {
    let record = family("busy", None, 1, Status::Working, 0).record;
    let mut transcript = Transcript::new();
    let conversation = || "c".to_owned();
    let tokens = |input, output| Tokens { input, output, cache_read: 0, cache_write: 0 };
    for signal in [
        TurnSignal::Started { conversation: conversation() },
        TurnSignal::Call { conversation: conversation(), call: "m1".into(), tokens: tokens(1_000, 20) },
        TurnSignal::Call { conversation: conversation(), call: "m1".into(), tokens: tokens(1_000, 200) },
        TurnSignal::Call { conversation: conversation(), call: "m2".into(), tokens: tokens(1_500, 40) },
    ] {
        transcript.tracker.observe(signal, 0);
    }
    let facts = SessionFacts::of(&record, Some(&transcript));
    assert_eq!(facts.live_tokens, 2_740);
    let mut pane = DigestPane::default();
    pane.facts.insert("busy".into(), facts);
    let summary = family("busy", None, 1, Status::Working, 0);
    let text = |pane: &DigestPane| digest_lines(pane, std::slice::from_ref(&summary), 0, 120).iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n");
    assert!(text(&pane).contains("2740 tokens"), "{}", text(&pane));
    pane.books = Digest { spend: vec![SessionSpend { session: "busy".into(), tokens: 10_000, api_usd: Some(0.5) }], ..Digest::default() };
    assert!(text(&pane).contains("12.7k tokens"), "{}", text(&pane));
}
