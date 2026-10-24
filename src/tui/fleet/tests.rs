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
use super::digest_view::header_lines;
use super::form::{Field, FormOutcome, LaunchForm};
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
        self.record(Command::Launch(Box::new(request.clone())))
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

#[test]
fn a_tui_launch_inherits_its_session_as_parent_only_when_present() {
    let accounts = vec![account("claude", "a")];
    let request = |parent| {
        let mut form = LaunchForm::new(accounts.clone(), PathBuf::from("/tmp/project").as_path(), &[], parent);
        form.field = Field::Prompt;
        form.paste("do the work");
        match form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            FormOutcome::Launch(request) => request,
            _ => panic!("valid launch form did not submit"),
        }
    };
    assert_eq!(request(Some("boss".into())).parent.as_deref(), Some("boss"));
    assert_eq!(request(None).parent, None);
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
    let runtimes = crate::fleet::runtime::Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]);
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

#[test]
fn a_pending_message_waits_for_its_own_echo() {
    let mut desk = desk();
    let mut screen = screen(&desk);
    type_text(&mut desk, &mut screen, "/compact");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let entries = &mut desk.transcripts.get_mut("parser").unwrap().entries;
    entries.push(Entry { kind: EntryKind::User, text: "[from child · turn 1] report".into() });
    screen.reconcile(|name| desk.transcript(name));
    assert_eq!(screen.pending_for("parser"), ["/compact"]);
    let entries = &mut desk.transcripts.get_mut("parser").unwrap().entries;
    entries.push(Entry { kind: EntryKind::Notice, text: "Compacted".into() });
    entries.push(Entry { kind: EntryKind::User, text: "/compact".into() });
    screen.reconcile(|name| desk.transcript(name));
    assert!(screen.pending.is_empty());
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
        verification: false,
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
    rendered_at(desk, screen, now_ms, 160)
}

fn rendered_at(desk: &DeskFake, screen: &mut Screen, now_ms: u64, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
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
        spend: vec![SessionSpend { session: "parser".into(), tokens: 12_345, api_usd: Some(0.0048) }],
        landed: vec![Landing { task_ref: "T1".into(), sha: "abcdef1234567".into(), wall_ms: Some(720_000), landed_ms: 0 }],
    };
    let mut screen = screen(&desk);
    observe(&desk, &mut screen);
    assert!(rendered(&desk, &mut screen, 0).contains("^T digest"));
    key(&mut desk, &mut screen, control('t'));
    let shown = rendered(&desk, &mut screen, 36_000);
    let at = |needle: &str| shown.find(needle).unwrap_or_else(|| panic!("{needle:?} missing from\n{shown}"));
    assert!(at("gate 3 · idle-one · default in 25s") < at("parser working 0m · up 36s"));
    assert!(at("parser working 0m · up 36s") < at("12.3k tokens · api $0.0048 · report: Looking at **parser.rs** now."));
    assert!(at("landed today") < at("T1 abcdef123 · wall 12m"));
    assert!(at("claude work 5h 37% left") < at("codex plus unmeasured"));
}

#[test]
fn the_header_wraps_at_account_boundaries_so_every_account_shows() {
    let account = |provider: &str, label: &str| Account { provider: ProviderName::new(provider), label: label.into(), home: PathBuf::new() };
    let accounts = [
        account("claude", "a"),
        account("claude", "c"),
        account("claude", "native"),
        account("codex", "pooled"),
        account("codex", "personal-azebaanbusiness-gmail"),
        account("codex", "personal-tom-passarelli-pm"),
        account("codex", "personal-tom-passarelli-proton"),
        account("codex", "personal-tompas0x-gmail"),
    ];
    let quota = Quota { windows: vec![QuotaWindow { label: "5h".into(), used_percent: 37 }, QuotaWindow { label: "7d opus".into(), used_percent: 4 }] };
    let lines = header_lines(&accounts, |_| Some(quota.clone()), 160);
    let text: Vec<String> = lines.iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect()).collect();
    assert!(lines.len() > 1 && lines.iter().all(|line| line.width() <= 160), "{text:#?}");
    for account in &accounts {
        assert!(text.iter().any(|line| line.contains(&format!("{} 5h 63% 7d opus 96% left", account.label))), "{} missing from {text:#?}", account.label);
    }
    assert_eq!(header_lines(&accounts[..2], |_| None, 160).len(), 1);
}

#[test]
fn a_resolved_gate_shows_the_recorded_answer_as_information_without_redelivery() {
    use crate::fleet::gates::{Answerer, Closure, GateBook, settle};
    let directory = tempfile::tempdir().unwrap();
    let ledger = crate::ledger::Ledger::open(&directory.path().join("ledger.db")).unwrap();
    let id = ledger.open_gate(&gate(18, "parser", 0).request, 0).unwrap();
    let closure = Closure { answer: "already chosen".into(), by: Answerer::Session("parent".into()), closed_ms: 1 };
    ledger.close_gate(id, &closure).unwrap();
    let outcome = settle(&ledger, id, Decision::Option(2), 2, |_, _| panic!("an existing answer must never be delivered twice"));
    let desk = desk();
    let mut screen = screen(&desk);
    screen.settle(Command::Decide { gate: id, session: "parser".into(), decision: Decision::Option(2) }, outcome.map(drop));
    let expected = format!("gate {id} was already answered: already chosen by parent");
    assert!(matches!(&screen.notice, Some(super::screen::Notice::Info(text)) if text == &expected));
    let text = rendered(&desk, &mut screen, 2);
    assert!(text.contains(&expected), "{text}");
    assert!(!text.contains("Codex rejected"));
    assert_eq!(ledger.gate(id).unwrap().unwrap().closure, Some(closure));
}

#[test]
fn the_key_hints_end_on_a_whole_hint_in_a_narrow_window() {
    let desk = desk();
    let mut screen = screen(&desk);
    let page = rendered_at(&desk, &mut screen, 0, 60);
    let footer = page.lines().last().unwrap();
    assert_eq!(footer, "Enter send · Alt+Enter newline · ^Enter/Alt+U urgent");
}
