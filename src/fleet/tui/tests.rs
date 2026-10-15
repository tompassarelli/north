use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::error::NorthResult;
use crate::fleet::model::{Account, Delivery, Entry, EntryKind, LaunchRequest, Provider, Status, Transcript};

use super::desk::{SessionActions, SessionDesk, SessionSummary};
use super::runner::Runner;
use super::screen::{Command, Dialog, Focus, Screen};
use super::{apply_event, drive};

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
        self.record(Command::Send { name: name.into(), text: text.into(), delivery })
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
}

#[derive(Default)]
struct DeskFake {
    accounts: Vec<Account>,
    sessions: Vec<SessionSummary>,
    transcripts: BTreeMap<String, Transcript>,
    actions: Arc<ActionsFake>,
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

    fn actions(&self) -> Arc<dyn SessionActions> {
        self.actions.clone()
    }
}

fn account(provider: Provider, label: &str) -> Account {
    Account { provider, label: label.into(), home: PathBuf::from("/accounts").join(label) }
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
        accounts: vec![account(Provider::Claude, "work"), account(Provider::Codex, "plus"), account(Provider::Claude, "home")],
        sessions: vec![summary("idle-one", Status::Idle, 9), summary("parser", Status::Working, 1), summary("done", Status::Exited, 50)],
        transcripts: BTreeMap::from([("parser".to_owned(), transcript)]),
        actions: Arc::default(),
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
    assert_eq!(desk.performed(), [Command::Send { name: "parser".into(), text: "stop and look".into(), delivery: Delivery::Now }]);
}
