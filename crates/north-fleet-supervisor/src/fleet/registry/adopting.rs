use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role, SessionRecord, Status};
use crate::fleet::prompt::PromptMode;
use crate::fleet::records::write_record;
use crate::fleet::host::{SOCKET, alive};
use crate::fleet::runtime::{LiveWindow, PROXY, Runtime, WindowInbox, now_ms};
use crate::fleet::store::{Claim, SessionRow};

use super::takeover::Predecessor;
use super::Fleet;

const WINDOW_CLOSE: Duration = Duration::from_secs(10);

enum Others {
    HangUp,
    Keep,
}

impl Fleet {
    pub fn adopt(&self, name: &str, conversation: &str, cwd: PathBuf) -> NorthResult<SessionRecord> {
        self.bind_window(name, conversation, cwd, Others::HangUp)
    }

    /// Binds `name` to the newest window on `conversation` as `adopt` does, but leaves the older window on it running, so a window move ends the window it moves from only once this binding holds.
    pub fn adopt_moved(&self, name: &str, conversation: &str, cwd: PathBuf) -> NorthResult<SessionRecord> {
        self.bind_window(name, conversation, cwd, Others::Keep)
    }

    fn bind_window(&self, name: &str, conversation: &str, cwd: PathBuf, others: Others) -> NorthResult<SessionRecord> {
        let (runtime, inbox) = self.window_of(conversation).ok_or_else(|| NorthError::State(format!("no live interactive window holds conversation {conversation}")))?;
        runtime.adopt_window(conversation, &inbox)?;
        let session_dir = self.session_dir(name)?;
        let placement = format!("adopted window, process {}", inbox.pid);
        if let Ok(held) = self.record(name)
            && held.adopted()
            && held.conversation.as_deref() == Some(conversation)
        {
            if held.account == inbox.account && held.placement.as_deref() == Some(placement.as_str()) {
                bar_hosts(&session_dir)?;
                return Ok(held);
            }
            if matches!(others, Others::HangUp) {
                self.hang_up_other_windows(conversation, inbox.pid)?;
            }
        }
        let role = if name == PROXY { Role::Proxy } else { Role::Lead };
        let request = LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name: name.to_owned(),
            provider: runtime.provider(),
            account: inbox.account,
            cwd,
            model: None,
            effort: None,
            prompt: String::new(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::default(),
            idle_exit_minutes: None,
            parent: None,
            task_ref: None,
            claim_box: None,
            placement: Some(placement),
            machine: None,
            capacity_need: None,
            role: Some(role),
            resume: None,
            compact_window: None,
            dispatched: false,
            surface: self.record(name).map(|held| held.surface).unwrap_or_default(),
            route: None,
                    };
        let mut record = request.record(None, Some(conversation.to_owned()), false, now_ms());
        record.adopted_ms = Some(record.created_ms);
        let row = SessionRow { status: Status::Idle.label().to_owned(), ..self.table_row(&record, record.created_ms) };
        self.registrar.register(&row, Claim::TakeOver)?;
        self.succeed(name, Claim::TakeOver, |predecessor| {
            record.addressed_since_ms = predecessor.as_ref().map(Predecessor::addressed_since);
            fs::create_dir_all(&session_dir)?;
            write_record(&session_dir, &record)?;
            bar_hosts(&session_dir)?;
            Ok(record)
        })
    }

    pub fn follow_conversation(&self, name: &str, conversation: &str) -> NorthResult<()> {
        let mut record = self.record(name)?;
        if record.conversation.as_deref() == Some(conversation) {
            return Ok(());
        }
        record.conversation = Some(conversation.to_owned());
        write_record(&self.session_dir(name)?, &record)
    }

    pub fn caller_conversation(&self) -> Option<String> {
        self.runtimes.caller_conversation()
    }

    pub(super) fn window_of(&self, conversation: &str) -> Option<(Arc<dyn Runtime>, WindowInbox)> {
        let accounts = self.accounts();
        self.providers().into_iter().filter_map(|provider| self.runtimes.get(&provider).ok()).find_map(|runtime| {
            let own: Vec<_> = accounts.iter().filter(|account| account.provider == runtime.provider()).cloned().collect();
            let newest = runtime.windows(&own).into_iter().filter(|window| window.conversation == conversation).max_by_key(|window| window.started_ms).map(|window| window.inbox);
            newest.or_else(|| runtime.window_inbox(&own, conversation)).map(|inbox| (runtime, inbox))
        })
    }

    fn hang_up_other_windows(&self, conversation: &str, keep: u32) -> NorthResult<()> {
        let others: Vec<LiveWindow> = self.live_windows().into_iter().filter(|window| window.conversation == conversation && window.inbox.pid != keep).collect();
        others.iter().try_for_each(|window| hang_up(&window.inbox))
    }

    pub(super) fn window_status(&self, record: &SessionRecord) -> Status {
        match record.conversation.as_deref().and_then(|conversation| self.window_of(conversation)) {
            Some((_, inbox)) if !record.ended() && inbox.busy => Status::Working,
            Some(_) if !record.ended() => Status::Idle,
            _ => Status::Exited,
        }
    }

    /// Hangs up the interactive window holding an adopted session and waits until its process is gone; false when no window held it.
    pub fn close_window(&self, record: &SessionRecord) -> NorthResult<bool> {
        let Some((_, inbox)) = record.conversation.as_deref().and_then(|conversation| self.window_of(conversation)) else { return Ok(false) };
        hang_up(&inbox)?;
        Ok(true)
    }

    pub(super) fn tell_window(&self, record: &SessionRecord, text: &str) -> NorthResult<()> {
        let gone = || NorthError::State(format!("{} is adopted by an interactive window that is no longer running; run north fleet adopt --name {} from the window that should hold it", record.name, record.name));
        if record.ended() {
            return Err(gone());
        }
        let (runtime, inbox) = record.conversation.as_deref().and_then(|conversation| self.window_of(conversation)).ok_or_else(gone)?;
        runtime.tell_window(&inbox, text)
    }
}

impl Fleet {
    pub fn adopt_if_free(&self, name: &str, conversation: &str, cwd: PathBuf) -> NorthResult<Option<SessionRecord>> {
        if !self.live_windows().iter().any(|window| window.conversation == conversation) {
            return Ok(None);
        }
        if let Ok(held) = self.record(name)
            && held.conversation.as_deref() != Some(conversation)
            && self.holds(&held)
        {
            return Ok(None);
        }
        self.adopt(name, conversation, cwd).map(Some)
    }

    /// Binds a proxy name whose holder is dead to the free live window on the holder's own conversation, so a window that resumed the proxy holds the name without anyone running adopt; with no such window nothing is bound.
    pub(super) fn rebind_dead_proxy(&self, name: &str) -> NorthResult<()> {
        let held = self.record(name)?;
        if held.role() != Role::Proxy || self.holds(&held) {
            return Ok(());
        }
        let bound: Vec<String> = self.sessions()?.into_iter().filter(|record| record.adopted() && self.holds(record)).filter_map(|record| record.conversation).collect();
        let own = self.live_windows().into_iter().find(|window| held.conversation.as_deref() == Some(window.conversation.as_str()) && !bound.contains(&window.conversation));
        match own {
            Some(window) => self.adopt(name, &window.conversation, window.cwd).map(drop),
            None => Ok(()),
        }
    }

    fn live_windows(&self) -> Vec<LiveWindow> {
        let accounts = self.accounts();
        let mut windows: Vec<LiveWindow> = self
            .providers()
            .into_iter()
            .filter_map(|provider| self.runtimes.get(&provider).ok())
            .flat_map(|runtime| {
                let own: Vec<_> = accounts.iter().filter(|account| account.provider == runtime.provider()).cloned().collect();
                runtime.windows(&own)
            })
            .collect();
        windows.sort_by_key(|window| std::cmp::Reverse(window.started_ms));
        windows
    }

    pub(super) fn refuse_on_window(&self, conversation: Option<&str>) -> NorthResult<()> {
        let Some(conversation) = conversation else { return Ok(()) };
        match self.live_windows().iter().find(|window| window.conversation == conversation) {
            Some(window) => Err(NorthError::Refused(format!("conversation {conversation} is open in an interactive window (process {}); the fleet starts no host on it", window.inbox.pid))),
            None => Ok(()),
        }
    }

    fn holds(&self, record: &SessionRecord) -> bool {
        !matches!(self.status(record), Status::Exited | Status::Parked)
    }
}

fn hang_up(inbox: &WindowInbox) -> NorthResult<()> {
    let pid = i32::try_from(inbox.pid).ok().and_then(Pid::from_raw).ok_or_else(|| NorthError::State(format!("window process {} has no pid", inbox.pid)))?;
    let _ = kill_process(pid, Signal::Hup);
    let deadline = Instant::now() + WINDOW_CLOSE;
    while alive(inbox.pid) {
        if Instant::now() > deadline {
            return Err(NorthError::Refused(format!("window process {} was still running {} s after it was hung up", inbox.pid, WINDOW_CLOSE.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn bar_hosts(session_dir: &Path) -> NorthResult<()> {
    let socket = session_dir.join(SOCKET);
    if !socket.is_dir() {
        let _ = fs::remove_file(&socket);
        fs::create_dir(&socket)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::Value;

    use super::*;
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::model::{Account, ProviderName, Quota, Transcript};
    use crate::fleet::roster::Roster;
    use crate::fleet::runtime::{HostSpec, Instructions, Opening};
    use crate::fleet::runtimes::Runtimes;
    use crate::fleet::turns::TurnSignal;

    const HOLDER: &str = "conversation-of-the-dead-proxy";
    const OTHER: &str = "conversation-of-an-empty-window";

    struct Windows {
        live: Mutex<Vec<(String, u32)>>,
    }

    impl Runtime for Windows {
        fn provider(&self) -> ProviderName {
            Stream.provider()
        }

        fn models(&self) -> &'static [&'static str] {
            Stream.models()
        }

        fn roster(&self, _state_root: &Path, home: &Path, _policy: &Path) -> Roster {
            Roster { accounts: vec![Account { provider: Stream.provider(), label: "c".into(), home: home.join("claude-c"), token_file: None, api: None }], ..Roster::default() }
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

        fn windows(&self, _accounts: &[Account]) -> Vec<LiveWindow> {
            self.live.lock().unwrap().iter().map(|(conversation, pid)| LiveWindow { conversation: conversation.clone(), started_ms: 1, cwd: "/tmp".into(), inbox: WindowInbox { account: "c".into(), pid: *pid, socket: PathBuf::new(), token: "peer".into(), busy: false } }).collect()
        }
    }

    #[test]
    fn rebind_dead_proxy_binds_no_window_but_the_holders_own_conversation() {
        let home = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Windows { live: Mutex::new(vec![(OTHER.to_owned(), 4102)]) });
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![runtime.clone()]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()));
        let request: LaunchRequest = serde_json::from_value(serde_json::json!({
            "verification": false, "name": PROXY, "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "prompt": "", "system_prompt": null,
            "foreground_cap_secs": null, "prompt_mode": "north", "idle_exit_minutes": null, "parent": null, "task_ref": null, "placement": null, "machine": null, "role": "proxy"
        }))
        .unwrap();
        let session_dir = fleet.session_dir(PROXY).unwrap();
        fs::create_dir_all(&session_dir).unwrap();
        write_record(&session_dir, &request.record(None, Some(HOLDER.to_owned()), false, 0)).unwrap();

        fleet.rebind_dead_proxy(PROXY).unwrap();
        assert!(!fleet.record(PROXY).unwrap().adopted(), "a window on another conversation never takes a dead proxy's name");

        runtime.live.lock().unwrap().push((HOLDER.to_owned(), 4103));
        fleet.rebind_dead_proxy(PROXY).unwrap();
        let held = fleet.record(PROXY).unwrap();
        assert_eq!((held.conversation.as_deref(), held.placement.as_deref()), (Some(HOLDER), Some("adopted window, process 4103")));
    }
}
