use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::Digest;
use crate::fleet::gates::{Decision, Gate, GateRequest};
use crate::fleet::model::{Account, Delivery, Effort, Entry, EntryKind, LaunchRequest, ProviderName, SessionPhase, Status, Transcript};

use super::clipboard::Clipboard;
use super::desk::{SessionActions, SessionDesk, SessionSummary};
use super::digest::SessionFacts;
use super::form::{Field, FormOutcome, LaunchForm};
use super::runner::Runner;
use super::screen::{Command, Dialog, Focus, Notice, Screen};
use super::{apply_event, drive, slash};

const HANG: Duration = Duration::from_secs(3600);

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
            gate.recv_timeout(HANG).unwrap();
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
    Account { provider: ProviderName::new(provider), label: label.into(), home: PathBuf::from("/accounts").join(label), token_file: None, api: None }
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
    let ledger = Arc::new(north_providers::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
    let runtimes = crate::fleet::runtimes::Runtimes::new(vec![Arc::new(north_providers::providers::claude::fleet::Claude)]);
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
        phase: SessionPhase::Live(status),
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
    let mut completions = Vec::new();
    assert!(north_base::platform::wait::until(HANG, || {
        completions = runner.completed();
        !completions.is_empty()
    }), "command did not complete");
    for completion in completions {
        screen.settle(completion.command, completion.outcome);
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
fn a_slash_command_the_screen_lacks_is_refused_and_named_not_sent() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    type_text(&mut desk, &mut screen, "/compact");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert!(desk.performed().is_empty());
    assert!(matches!(&screen.notice, Some(Notice::Error(text)) if text.contains("/model") && text.contains("/effort")));
    assert!(screen.composer.is_empty());
}

#[test]
fn every_command_the_help_lists_has_a_handler() {
    for (name, _) in slash::COMMANDS {
        let mut desk = desk();
        let mut screen = screen(&desk);
        type_text(&mut desk, &mut screen, name);
        key(&mut desk, &mut screen, press(KeyCode::Enter));
        assert!(desk.performed().is_empty(), "{name} reached the client");
        assert!(matches!(&screen.notice, Some(Notice::Info(_))) || screen.help.tab().is_some(), "{name} has no handler");
    }
}

#[test]
fn a_launch_settling_keeps_the_session_selected_while_typing() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    key(&mut desk, &mut screen, control('n'));
    let Some(Dialog::Launch(form)) = &mut screen.dialog else { panic!("Ctrl+N opened no launch form") };
    form.field = Field::Prompt;
    screen.paste("do the work");
    let launch = apply_event(&mut screen, Event::Key(press(KeyCode::Enter))).expect("the launch form did not submit");
    screen.begin(&launch, None);
    desk.actions.record(launch.clone()).unwrap();
    screen.sync(desk.sessions());
    screen.focus = Focus::List;
    key(&mut desk, &mut screen, press(KeyCode::Down));
    let typed_to = screen.selected.clone().unwrap();
    assert_ne!(typed_to, launch.session());
    type_text(&mut desk, &mut screen, "hello");
    screen.settle(launch.clone(), Ok(()));
    assert_eq!(screen.selected.as_deref(), Some(typed_to.as_str()));
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert_eq!(desk.performed().last(), Some(&Command::Send { name: typed_to, text: "hello".into(), delivery: Delivery::Queued, deepen: false }));
    screen.begin(&launch, None);
    screen.settle(launch.clone(), Ok(()));
    assert_eq!(screen.selected.as_deref(), Some(launch.session()));
    assert_eq!(screen.focus, Focus::Composer);
    type_text(&mut desk, &mut screen, "x");
    screen.focus = Focus::List;
    screen.begin(&launch, None);
    screen.settle(launch.clone(), Ok(()));
    assert_eq!(screen.selected.as_deref(), Some(launch.session()));
    assert_eq!(screen.focus, Focus::List);
    assert_eq!(screen.composer.lines().concat(), "x");
}

#[test]
fn a_refused_launch_keeps_the_form_as_typed() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    key(&mut desk, &mut screen, control('n'));
    let Some(Dialog::Launch(form)) = &mut screen.dialog else { panic!("Ctrl+N opened no launch form") };
    form.field = Field::Model;
    type_text(&mut desk, &mut screen, "opus");
    let Some(Dialog::Launch(form)) = &mut screen.dialog else { panic!("typing closed the form") };
    form.field = Field::Prompt;
    screen.paste("do the work");
    let launch = apply_event(&mut screen, Event::Key(press(KeyCode::Enter))).expect("the launch form did not submit");
    let Command::Launch(request) = &launch else { panic!("the form sent {launch:?}") };
    let name = request.name.clone();
    screen.begin(&launch, None);
    assert!(screen.dialog.is_none());
    screen.settle(launch, Err(NorthError::Refused("account a has nothing left".into())));
    let Some(Dialog::Launch(form)) = &screen.dialog else { panic!("the refused launch left no form open") };
    assert_eq!(form.name.lines().concat(), name);
    assert_eq!(form.model.lines().concat(), "opus");
    assert_eq!(form.prompt.lines().concat(), "do the work");
    assert_eq!(form.problem.as_deref(), Some("account a has nothing left"));
}

#[test]
fn a_paste_with_carriage_returns_keeps_its_lines() {
    let desk = desk();
    let mut screen = screen(&desk);
    apply_event(&mut screen, Event::Paste("first line\rsecond line\r\nthird line".into()));
    assert_eq!(screen.composer.lines(), ["first line", "second line", "third line"]);
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
        ask_once: None,
    };
    Gate::new(id, request, opened_ms, None)
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
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let decided = |gate, decision| Command::Decide { gate, session: String::new(), decision };
    assert_eq!(desk.performed(), [decided(2, Decision::Option(1)), decided(1, Decision::Text("rent a smaller one".into()))]);
    assert!(screen.pending.is_empty(), "a gate answer is not a message to the selected session");
}

#[test]
fn a_worded_gate_answer_waits_for_confirmation() {
    let mut desk = desk();
    desk.digest.gates = vec![gate(1, "parser", 0)];
    let mut screen = screen(&desk);
    observe(&desk, &mut screen);
    key(&mut desk, &mut screen, control('t'));
    key(&mut desk, &mut screen, press(KeyCode::Tab));
    assert_eq!(screen.focus, Focus::Composer);
    let decided = |decision| Command::Decide { gate: 1, session: String::new(), decision };
    type_text(&mut desk, &mut screen, "1j");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert!(desk.performed().is_empty(), "free text is held, not sent");
    let shown = screen.confirmation().expect("the held answer is shown for confirmation");
    assert!(shown.contains("gate 1") && shown.contains("Rent the GPU box?") && shown.contains("1j"), "{shown}");
    key(&mut desk, &mut screen, press(KeyCode::Esc));
    assert!(screen.confirmation().is_none());
    assert_eq!(screen.composer.lines().join("\n"), "1j");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    key(&mut desk, &mut screen, press(KeyCode::Char('x')));
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert!(desk.performed().is_empty(), "an edit starts the confirmation over");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert_eq!(desk.performed(), [decided(Decision::Text("1jx".into()))]);
    type_text(&mut desk, &mut screen, "Wait");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    assert_eq!(desk.performed().last(), Some(&decided(Decision::Option(2))));
}


#[test]
fn a_copy_reaches_the_terminal_clipboard_without_wl_copy() {
    let desk = desk();
    let mut screen = screen(&desk).with_clipboard(Clipboard::with_tools("north-test-absent-wl-copy", "north-test-absent-wl-paste"));
    let facts = SessionFacts { reply: Some("two words".into()), ..SessionFacts::default() };
    screen.observe(Digest::default(), BTreeMap::from([("parser".to_owned(), facts)]));
    screen.selected = Some("parser".into());
    apply_event(&mut screen, Event::Key(control('y')));
    assert_eq!(screen.take_terminal_output(), "\x1b]52;c;dHdvIHdvcmRz\x07");
    assert!(matches!(&screen.notice, Some(Notice::Info(text)) if text.contains("through the terminal")));
    apply_event(&mut screen, Event::Key(control('v')));
    assert!(matches!(&screen.notice, Some(Notice::Error(text)) if text.contains("needs north-test-absent-wl-paste") && text.contains("not installed")));
}
