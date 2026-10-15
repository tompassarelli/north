use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::error::NorthResult;
use crate::fleet::books::{Digest, Landing, SessionSpend};
use crate::fleet::gates::{Decision, Gate, GateRequest};
use crate::fleet::model::{Account, Delivery, Entry, EntryKind, LaunchRequest, ProviderName, Quota, QuotaWindow, Status, Transcript};
use crate::fleet::prompt::deepened;

use super::desk::{SessionActions, SessionDesk, SessionSummary};
use super::digest::SessionFacts;
use super::form::Field;
use super::painter::TranscriptPainter;
use super::runner::Runner;
use super::screen::{Command, Dialog, Focus, Screen};
use super::{apply_event, drive, view};

#[derive(Default)]
struct ActionsFake {
    performed: Mutex<Vec<Command>>,
    stop_gate: Mutex<Option<Receiver<()>>>,
}

impl ActionsFake {
    fn record(&self, command: Command) -> NorthResult<()> {
        self.performed.lock().unwrap().push(command);
        Ok(())
    }
}

impl SessionActions for ActionsFake {
    fn launch(&self, request: &LaunchRequest) -> NorthResult<()> {
        self.record(Command::Launch(request.clone()))
    }

    fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.record(Command::Send { name: name.into(), text: text.into(), delivery, deepen: false })
    }

    fn revive(&self, name: &str) -> NorthResult<()> {
        self.record(Command::Revive(name.into()))
    }

    fn interrupt(&self, name: &str) -> NorthResult<()> {
        self.record(Command::Interrupt(name.into()))
    }

    fn stop(&self, name: &str) -> NorthResult<()> {
        if let Some(gate) = self.stop_gate.lock().unwrap().as_ref() {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        self.record(Command::Stop(name.into()))
    }

    fn forget(&self, name: &str) -> NorthResult<()> {
        self.record(Command::Forget(name.into()))
    }

    fn decide(&self, gate: i64, decision: Decision) -> NorthResult<()> {
        self.record(Command::Decide { gate, session: String::new(), decision })
    }
}

#[derive(Default)]
struct DeskFake {
    accounts: Vec<Account>,
    sessions: Vec<SessionSummary>,
    transcripts: BTreeMap<String, Transcript>,
    actions: Arc<ActionsFake>,
    digest: Digest,
}

impl DeskFake {
    fn performed(&self) -> Vec<Command> {
        self.actions.performed.lock().unwrap().clone()
    }
}

impl SessionDesk for DeskFake {
    fn accounts(&self) -> Vec<Account> {
        self.accounts.clone()
    }

    fn refresh(&mut self) -> NorthResult<bool> {
        Ok(true)
    }

    fn sessions(&self) -> Vec<SessionSummary> {
        let launched = self.performed().into_iter().filter_map(|command| match command {
            Command::Launch(request) => Some(summary(&request.name, Status::Starting, 0)),
            _ => None,
        });
        self.sessions.iter().cloned().chain(launched).collect()
    }

    fn transcript(&self, name: &str) -> Option<&Transcript> {
        self.transcripts.get(name)
    }

    fn digest(&self) -> Digest {
        self.digest.clone()
    }

    fn actions(&self) -> Arc<dyn SessionActions> {
        self.actions.clone()
    }
}

fn account(provider: &str, label: &str) -> Account {
    Account { provider: ProviderName::new(provider), label: label.into(), home: PathBuf::from("/accounts").join(label) }
}

fn summary(name: &str, status: Status, last_activity_ms: u64) -> SessionSummary {
    SessionSummary {
        record: serde_json::from_value(serde_json::json!({
            "name": name,
            "provider": "claude",
            "account": "work",
            "cwd": format!("/code/{name}"),
            "model": null,
            "created_ms": 0,
            "host_pid": null,
            "conversation": null,
        }))
        .unwrap(),
        status,
        last_activity_ms: Some(last_activity_ms),
    }
}

fn desk() -> DeskFake {
    let mut transcript = Transcript::new();
    transcript.status = Status::Working;
    transcript.entries = vec![
        Entry { kind: EntryKind::User, text: "fix the parser".into() },
        Entry { kind: EntryKind::Assistant, text: "Looking at **parser.rs** now.".into() },
        Entry { kind: EntryKind::Tool, text: "Read src/parser.rs".into() },
        Entry { kind: EntryKind::ToolResult, text: "fn parse() {}\nmore".into() },
        Entry { kind: EntryKind::Error, text: "rate limited".into() },
    ];
    DeskFake {
        accounts: vec![account("claude", "work"), account("codex", "plus"), account("claude", "home")],
        sessions: vec![summary("idle-one", Status::Idle, 9), summary("parser", Status::Working, 1), summary("done", Status::Exited, 50)],
        transcripts: BTreeMap::from([("parser".to_owned(), transcript)]),
        actions: Arc::default(),
        digest: Digest::default(),
    }
}

fn screen(desk: &DeskFake) -> Screen {
    let mut screen = Screen::new(PathBuf::from("/code/north"), desk.accounts());
    screen.sync(desk.sessions());
    screen
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn control(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

fn type_text(desk: &mut DeskFake, screen: &mut Screen, text: &str) {
    for character in text.chars() {
        key(desk, screen, press(KeyCode::Char(character)));
    }
}

fn key(desk: &mut DeskFake, screen: &mut Screen, event: KeyEvent) {
    let Some(command) = apply_event(screen, Event::Key(event)) else { return };
    screen.begin(&command, desk.transcript(command.session()));
    let mut runner = Runner::new(desk.actions());
    runner.start(command);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let completions = runner.completed();
        if !completions.is_empty() {
            for completion in completions {
                screen.settle(completion.command, completion.outcome);
            }
            return;
        }
        assert!(Instant::now() < deadline, "command did not complete");
        std::thread::yield_now();
    }
}

#[test]
fn stop_needs_confirmation_and_forget_needs_an_exited_session() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    key(&mut desk, &mut screen, control('d'));
    key(&mut desk, &mut screen, control('x'));
    assert!(matches!(screen.dialog, Some(Dialog::ConfirmStop(ref name)) if name == "parser"));
    key(&mut desk, &mut screen, press(KeyCode::Char('y')));
    key(&mut desk, &mut screen, control('c'));
    screen.focus = Focus::List;
    key(&mut desk, &mut screen, press(KeyCode::End));
    key(&mut desk, &mut screen, control('d'));
    assert_eq!(desk.performed(), [Command::Stop("parser".into()), Command::Interrupt("parser".into()), Command::Forget("done".into())]);
    assert_eq!(screen.selected.as_deref(), Some("parser"));
}

#[test]
fn a_blocking_stop_leaves_the_loop_responsive() {
    let mut desk = desk();
    let (release, gate) = channel();
    *desk.actions.stop_gate.lock().unwrap() = Some(gate);
    let mut screen = screen(&desk);
    let mut script: VecDeque<Event> = [control('x'), press(KeyCode::Char('y')), press(KeyCode::Char('h')), press(KeyCode::Char('i')), control('q')]
        .into_iter()
        .map(Event::Key)
        .collect();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    let actions = Arc::clone(&desk.actions);
    let mut events = |_| {
        if script.len() == 1 {
            assert!(actions.performed.lock().unwrap().is_empty());
            release.send(()).unwrap();
        }
        Ok(script.pop_front())
    };
    drive(&mut desk, &mut screen, &mut terminal, &mut events).unwrap();
    assert_eq!(screen.composer.lines().join(""), "hi");
    assert_eq!(desk.performed(), [Command::Stop("parser".into())]);
    assert!(screen.busy.is_empty());
}

#[test]
fn a_sent_message_shows_at_once_and_yields_to_its_echo() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    type_text(&mut desk, &mut screen, "run tests");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert_eq!(screen.pending_for("parser"), ["run tests"]);
    screen.reconcile(|name| desk.transcript(name));
    assert_eq!(screen.pending_for("parser"), ["run tests"]);
    desk.transcripts.get_mut("parser").unwrap().entries.push(Entry { kind: EntryKind::User, text: "run tests".into() });
    screen.reconcile(|name| desk.transcript(name));
    assert!(screen.pending.is_empty());
}

#[test]
fn alt_u_sends_urgently_to_a_parked_session() {
    let mut desk = desk();
    desk.sessions[1].status = Status::Parked;
    let mut screen = screen(&desk);
    for code in [KeyCode::Tab, KeyCode::Char('j'), KeyCode::Tab] {
        key(&mut desk, &mut screen, press(code));
    }
    assert_eq!(screen.selected.as_deref(), Some("parser"));
    type_text(&mut desk, &mut screen, "stop and look");
    key(&mut desk, &mut screen, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT));
    assert_eq!(desk.performed(), [Command::Send { name: "parser".into(), text: "stop and look".into(), delivery: Delivery::Now, deepen: false }]);
}

#[test]
fn the_launch_form_tabs_from_its_first_field() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    key(&mut desk, &mut screen, control('n'));
    for _ in 0..4 {
        key(&mut desk, &mut screen, press(KeyCode::Tab));
    }
    let Some(Dialog::Launch(form)) = &screen.dialog else { panic!("Ctrl+N opens the launch form") };
    assert_eq!(form.field, Field::ORDER[4]);
}

#[test]
fn ctrl_g_sends_the_draft_after_the_deepen_questions() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    type_text(&mut desk, &mut screen, "fix the parser");
    key(&mut desk, &mut screen, control('g'));
    let sent = deepened("fix the parser");
    assert_eq!(desk.performed(), [Command::Send { name: "parser".into(), text: sent, delivery: Delivery::Queued, deepen: false }]);
    assert_eq!(screen.pending_for("parser"), ["fix the parser"]);
    assert!(screen.composer.is_empty());
}

fn gate(id: i64, session: &str, opened_ms: u64) -> Gate {
    let request = GateRequest {
        session: session.into(),
        question: "Rent the GPU box?".into(),
        options: vec!["rent it".into(), "wait".into()],
        recommend: 1,
        default: 2,
        default_after_ms: 60_000,
    };
    Gate { id, request, opened_ms, closure: None }
}

fn observe(desk: &DeskFake, screen: &mut Screen) {
    let facts = screen.sessions.iter().map(|session| (session.record.name.clone(), SessionFacts::of(&session.record, desk.transcript(&session.record.name)))).collect();
    screen.observe(desk.digest(), facts);
}

fn rendered(desk: &DeskFake, screen: &mut Screen, now_ms: u64) -> String {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let transcript = desk.transcript("parser");
    terminal.draw(|frame| view::draw(frame, screen, transcript, &mut TranscriptPainter::default(), now_ms)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer.content.chunks(usize::from(buffer.area.width)).map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>().trim_end().to_owned()).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_digest_answers_the_selected_gate_with_an_option_or_free_text() {
    let mut desk = desk();
    desk.digest.gates = vec![gate(1, "parser", 0), gate(2, "idle-one", 10)];
    let mut screen = screen(&desk);
    observe(&desk, &mut screen);
    key(&mut desk, &mut screen, control('t'));
    assert_eq!(screen.focus, Focus::List);
    key(&mut desk, &mut screen, press(KeyCode::Char('j')));
    key(&mut desk, &mut screen, press(KeyCode::Char('1')));
    key(&mut desk, &mut screen, press(KeyCode::Char('9')));
    key(&mut desk, &mut screen, press(KeyCode::Char('k')));
    type_text(&mut desk, &mut screen, "rent a smaller one");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let decided = |gate, decision| Command::Decide { gate, session: String::new(), decision };
    assert_eq!(desk.performed(), [decided(2, Decision::Option(1)), decided(1, Decision::Text("rent a smaller one".into()))]);
    assert!(screen.pending.is_empty(), "a gate answer is not a message to the selected session");
}

#[test]
fn a_new_open_gate_rings_the_bell_and_marks_its_session_unread() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    observe(&desk, &mut screen);
    assert!(!screen.take_bell());
    desk.digest.gates = vec![gate(4, "idle-one", 0)];
    observe(&desk, &mut screen);
    assert!(screen.take_bell());
    assert!(screen.unread.contains("idle-one"));
    observe(&desk, &mut screen);
    assert!(!screen.take_bell(), "a gate already seen must not ring again");
}

#[test]
fn the_digest_lists_open_gates_first_then_sessions_and_todays_landings() {
    let mut desk = desk();
    let parser = desk.transcripts.get_mut("parser").unwrap();
    parser.entries.push(Entry { kind: EntryKind::TurnEnd, text: "success".into() });
    parser.entries.push(Entry { kind: EntryKind::User, text: "now add tests".into() });
    parser.quota = Some(Quota { windows: vec![QuotaWindow { label: "5h".into(), used_percent: 63 }] });
    desk.digest = Digest {
        gates: vec![gate(3, "idle-one", 1_000)],
        spend: vec![SessionSpend { session: "parser".into(), tokens: 12_345, api_usd: Some(0.5) }],
        landed: vec![Landing { task_ref: "T1".into(), sha: "abcdef1234567".into(), wall_ms: Some(720_000), landed_ms: 0 }],
    };
    let mut screen = screen(&desk);
    observe(&desk, &mut screen);
    assert!(rendered(&desk, &mut screen, 0).contains("^T digest"));
    key(&mut desk, &mut screen, control('t'));
    let shown = rendered(&desk, &mut screen, 36_000);
    let at = |needle: &str| shown.find(needle).unwrap_or_else(|| panic!("{needle:?} missing from\n{shown}"));
    assert!(at("gate 3 · idle-one · default in 25s") < at("parser working · fix the parser"));
    assert!(at("parser working · fix the parser") < at("12.3k tokens · api $0.50 · report: Looking at **parser.rs** now."));
    assert!(at("landed today") < at("T1 abcdef123 · wall 12m"));
    assert!(at("claude work 5h 37% left") < at("codex plus unmeasured"));
}
