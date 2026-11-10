use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rustix::process::{Pid, Signal, kill_process};
use serde_json::Value;

use super::proxy::{DEADLINE_MS, Desk};
use super::{AccountReadings, HandOver, Observed, hand_over};
use crate::error::NorthResult;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::host::alive;
use crate::fleet::model::{Account, ApiKey, LaunchRequest, ProviderName, Quota, SessionRecord, Transcript};
use crate::fleet::registry::Fleet;
use crate::fleet::roster::Roster;
use crate::fleet::runtime::{HostSpec, Instructions, LiveWindow, Opening, PROXY, Runtime, WindowInbox, now_ms};
use crate::fleet::runtimes::Runtimes;
use crate::fleet::turns::TurnSignal;
use crate::usage::decide::world::{Credit, Label, Percent, Reading, RowId, Source, Unix, Usd, Window};

const CONVERSATION: &str = "5b1d7c52-proxy-window";

struct Windowed {
    on: String,
    accounts: Vec<Account>,
    pid: u32,
    busy: AtomicBool,
    told: Mutex<Vec<String>>,
}

impl Windowed {
    fn inbox(&self) -> Option<WindowInbox> {
        alive(self.pid).then(|| WindowInbox { account: self.on.clone(), pid: self.pid, socket: PathBuf::new(), token: "peer".into(), busy: self.busy.load(Ordering::SeqCst) })
    }
}

impl Runtime for Windowed {
    fn provider(&self) -> ProviderName {
        Stream.provider()
    }

    fn models(&self) -> &'static [&'static str] {
        Stream.models()
    }

    fn roster(&self, _state_root: &Path, _home: &Path, _policy: &Path) -> Roster {
        Roster { accounts: self.accounts.clone(), ..Roster::default() }
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        Stream.assign_conversation()
    }

    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec> {
        Stream.host(account, request, opening, instructions)
    }

    fn handshake(&self) -> Vec<String> {
        Stream.handshake()
    }

    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening, instructions: Instructions) -> NorthResult<Vec<String>> {
        Stream.open(name, request, opening, instructions)
    }

    fn message(&self, transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
        Stream.message(transcript, id, text)
    }

    fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>> {
        Stream.interrupt(transcript)
    }

    fn close(&self, transcript: &Transcript) -> Vec<String> {
        Stream.close(transcript)
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        Stream.observe(frame, transcript);
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        Stream.signals(frame)
    }

    fn quota(&self, frame: &Value) -> Option<Quota> {
        Stream.quota(frame)
    }

    fn window_inbox(&self, _accounts: &[Account], conversation: &str) -> Option<WindowInbox> {
        self.inbox().filter(|_| conversation == CONVERSATION)
    }

    fn windows(&self, _accounts: &[Account]) -> Vec<LiveWindow> {
        self.inbox().map(|inbox| LiveWindow { conversation: CONVERSATION.into(), started_ms: 1, cwd: "/tmp".into(), inbox }).into_iter().collect()
    }

    fn tell_window(&self, _inbox: &WindowInbox, text: &str) -> NorthResult<()> {
        self.told.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

struct Table;

impl AccountReadings for Table {
    fn observed(&self, now_ms: u64) -> Observed {
        let now = (now_ms / 1000) as i64;
        let read = |row, account: &str, window, used| Reading { row: RowId(row), account: Label(account.into()), window, used: Percent(used), resets_at: Some(Unix(now + 2 * 86_400)), read_at: Unix(now - 60), source: Source::Header };
        Observed { readings: vec![read(0, "c", Window::FiveHour, 20.0), read(1, "c", Window::Week, 99.0), read(2, "b", Window::FiveHour, 5.0), read(3, "b", Window::Week, 7.0)], credits: [("gmail".to_owned(), Credit { left: Usd(0.0), next_grant: None })].into(), ..Observed::default() }
    }
}

struct Adopted {
    _home: tempfile::TempDir,
    fleet: Fleet,
    window: Arc<Windowed>,
    record: SessionRecord,
    launches: RefCell<Vec<LaunchRequest>>,
}

impl Adopted {
    fn on_account_c() -> Self {
        Self::on("c")
    }

    fn on(on: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut child = Command::new("sleep").arg("600").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let pid = child.id();
        std::thread::spawn(move || child.wait());
        let account = |label: &str| Account { provider: Stream.provider(), label: label.into(), home: home.path().join(format!("claude-{label}")), token_file: None, api: label.split_once(':').map(|(_, org)| ApiKey { org: org.into(), file: home.path().join(org) }) };
        let window = Arc::new(Windowed { on: on.to_owned(), accounts: vec![account("b"), account("c"), account("api:gmail")], pid, busy: AtomicBool::new(false), told: Mutex::new(Vec::new()) });
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![window.clone()]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()));
        let record = fleet.adopt(PROXY, CONVERSATION, "/tmp".into()).unwrap();
        assert!(record.host_pid.is_none() && record.account == on);
        Self { _home: home, fleet, window, record, launches: RefCell::new(Vec::new()) }
    }

    fn tick(&self, now_ms: u64) -> HandOver {
        let take_over = |request: &LaunchRequest| {
            self.launches.borrow_mut().push(request.clone());
            Ok(self.record.clone())
        };
        hand_over(&self.fleet, &Table, &Default::default(), &|_| true, &take_over, &|_| Ok(()), now_ms)
    }

    fn desk(&self) -> Desk {
        Desk::of(&self.fleet.root, PROXY)
    }
}

impl Drop for Adopted {
    fn drop(&mut self) {
        if let Some(pid) = i32::try_from(self.window.pid).ok().and_then(Pid::from_raw) {
            let _ = kill_process(pid, Signal::Kill);
        }
    }
}

#[test]
fn an_adopted_proxy_window_on_an_account_at_99_percent_of_its_week_moves_to_the_7_percent_account_with_the_hand_over_it_writes_else_resumes_there_after_the_deadline() {
    let proxy = Adopted::on_account_c();
    let asked_ms = now_ms();

    let asked = proxy.tick(asked_ms);

    let told = proxy.window.told.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "{:?}", asked.lines);
    assert!(told[0].contains(&proxy.desk().file().display().to_string()) && told[0].contains("to account b"), "{}", told[0]);
    assert!(asked.lines.iter().any(|line| line.contains("asked for its hand-over") && line.ends_with("from account c to b")), "{:?}", asked.lines);
    assert!(proxy.launches.borrow().is_empty());

    proxy.window.busy.store(true, Ordering::SeqCst);
    std::fs::write(proxy.desk().file(), "In flight: north#826 lands through safe-push.").unwrap();
    let mid_turn = proxy.tick(asked_ms + 60_000);
    assert!(proxy.launches.borrow().is_empty() && alive(proxy.window.pid), "a turn in progress is never cut short: {:?}", mid_turn.lines);

    proxy.window.busy.store(false, Ordering::SeqCst);
    let moved = proxy.tick(asked_ms + 2 * 60_000);

    let launches = proxy.launches.borrow();
    assert_eq!(launches.len(), 1, "{:?}", moved.lines);
    assert_eq!((launches[0].name.as_str(), launches[0].account.as_str(), launches[0].resume.as_deref()), (PROXY, "b", None));
    assert!(launches[0].prompt.ends_with("In flight: north#826 lands through safe-push."), "{}", launches[0].prompt);
    assert!(!alive(proxy.window.pid), "the old window is stopped");
    assert!(!proxy.desk().file().exists() && proxy.fleet.root.join(format!(".handovers/{PROXY}-{asked_ms}.md")).exists());
    assert!(moved.lines.iter().any(|line| line.contains("handed over from account c to b") && line.ends_with("its old window is closed")), "{:?}", moved.lines);
    assert_eq!(proxy.window.told.lock().unwrap().len(), 1, "asked once");

    let silent = Adopted::on_account_c();
    let asked_ms = now_ms();
    silent.tick(asked_ms);

    let early = silent.tick(asked_ms + DEADLINE_MS - 1);
    assert!(silent.launches.borrow().is_empty(), "{:?}", early.lines);

    let resumed = silent.tick(asked_ms + DEADLINE_MS);

    let launches = silent.launches.borrow();
    assert_eq!(launches.len(), 1, "{:?}", resumed.lines);
    assert_eq!((launches[0].account.as_str(), launches[0].resume.as_deref()), ("b", Some(CONVERSATION)));
    assert!(!alive(silent.window.pid), "the window holding the conversation closes before it resumes elsewhere");
    assert!(resumed.lines.iter().any(|line| line.contains("resumes on account b instead of c")), "{:?}", resumed.lines);
}

#[test]
fn a_session_on_an_api_org_north_reads_as_spent_is_not_handed_over_to_a_subscription_account() {
    let session = Adopted::on("api:gmail");

    let ticked = session.tick(now_ms());

    assert!(session.launches.borrow().is_empty() && session.window.told.lock().unwrap().is_empty(), "{:?}", ticked.lines);
    assert!(alive(session.window.pid));
}
