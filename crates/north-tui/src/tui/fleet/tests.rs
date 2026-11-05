use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::error::NorthResult;
use crate::fleet::books::Digest;
use crate::fleet::gates::{Decision, Gate, GateRequest};
use crate::fleet::model::{Account, Delivery, Effort, Entry, EntryKind, LaunchRequest, ProviderName, Status, Transcript};

use super::desk::{SessionActions, SessionDesk, SessionSummary};
use super::digest::SessionFacts;
use super::form::{Field, FormOutcome, LaunchForm};
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
        self.record(Command::Launch(Box::new(request.clone())))
    }

    fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.record(Command::Send { name: name.into(), text: text.into(), delivery, deepen: false })
    }

    fn revive(&self, name: &str) -> NorthResult<()> {
        self.record(Command::Revive(name.into()))
    }

    fn relaunch(&self, name: &str, model: Option<&str>, effort: Option<Effort>) -> NorthResult<()> {
        self.record(Command::Relaunch { name: name.into(), model: model.map(str::to_owned), effort })
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
    Account { provider: ProviderName::new(provider), label: label.into(), home: PathBuf::from("/accounts").join(label), token_file: None }
}

#[test]
fn a_tui_launch_on_a_label_logged_into_another_labels_account_is_refused_naming_both() {
    let home = tempfile::tempdir().unwrap();
    for label in ["a", "b"] {
        let profile = home.path().join(format!(".local/state/north/profiles/claude-{label}"));
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join(".credentials.json"), "{}").unwrap();
        std::fs::write(profile.join(".claude.json"), r#"{"oauthAccount": {"accountUuid": "one-login"}}"#).unwrap();
    }
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
    let runtimes = crate::fleet::runtimes::Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]);
    let fleet = crate::fleet::registry::Fleet::at(home.path().into(), runtimes, ledger).unwrap();
    assert_eq!(fleet.accounts().iter().map(|account| account.label.as_str()).collect::<Vec<_>>(), ["a"]);

    let mut form = LaunchForm::new(vec![account("claude", "b")], home.path(), &[], None);
    form.field = Field::Prompt;
    form.paste("do the work");
    let FormOutcome::Launch(request) = form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) else { panic!("valid launch form did not submit") };
    let actions: Arc<dyn SessionActions> = Arc::new(fleet);
    match actions.launch(&request) {
        Err(crate::error::NorthError::Refused(why)) => assert!(why.starts_with("claude:b is logged in to the same account as claude:a"), "{why}"),
        other => panic!("a TUI launch on a shared login was not refused: {other:?}"),
    }
    assert!(!home.path().join(format!(".local/state/north/fleet/{}", request.name)).exists(), "the refused launch left a session");
}

fn summary(name: &str, status: Status, last_activity_ms: u64) -> SessionSummary {
    SessionSummary {
        record: serde_json::from_value(serde_json::json!({
            "name": name,
            "provider": "claude",
            "account": "work",
            "cwd": format!("/code/{name}"),
            "model": null,
            "created_ms": last_activity_ms,
            "host_pid": null,
            "conversation": null,
        }))
        .unwrap(),
        status,
        last_activity_ms: Some(last_activity_ms),
        since_ms: 0,
        depth: 0,
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
fn one_enter_sends_a_slash_command_while_its_suggestions_are_open() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    type_text(&mut desk, &mut screen, "/compact");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    type_text(&mut desk, &mut screen, "/comp");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let compact = Command::Send { name: "parser".into(), text: "/compact".into(), delivery: Delivery::Queued, deepen: false };
    assert_eq!(desk.performed(), [compact.clone(), compact]);
    assert_eq!(screen.pending_for("parser"), ["/compact", "/compact"]);
    assert!(screen.composer.is_empty());
}

fn gate(id: i64, session: &str, opened_ms: u64) -> Gate {
    let request = GateRequest {
        verification: false,
        issue: None,
        box_text: None,
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

