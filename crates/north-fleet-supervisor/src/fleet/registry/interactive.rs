use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::model::{Entry, LaunchRequest, SessionRecord, Status};
use crate::fleet::runtime::HostSpec;
use crate::fleet::unaccepted::Unaccepted;

use super::Follower;
use crate::fleet::detached;
use super::{Fleet, session_running};
use super::stopping::Halt;

const TURN_POLL: Duration = Duration::from_millis(500);
/// Looks at an idle host before an after-turn window takes it, so the message that woke the host a moment before the window opened starts its turn first.
const QUIET_LOOKS: u32 = 6;

impl Fleet {
    /// Hands a session to its provider's own interactive UI: with `after_turn`, first shows the turn the host is running through `show` as it streams and waits until it has ended and no background task its CLI started still runs, instead of cutting either off (stopping the host kills them); stops the fleet host and waits until it has exited, so the fleet host and the UI never hold one conversation together, then passes the interactive command to `launch`. The record stays stopped, so nothing in the fleet restarts it; `revive` takes it back once the UI is closed.
    pub fn interactive(&self, name: &str, after_turn: bool, show: impl FnMut(&Entry), launch: impl FnOnce(&HostSpec) -> NorthResult<()>) -> NorthResult<()> {
        self.interactive_pacing(name, after_turn, || sleep(TURN_POLL), show, launch)
    }

    /// `interactive` with the pause between two looks at the running turn handed in, so a caller that owns the turn can end it between the looks.
    pub fn interactive_pacing(&self, name: &str, after_turn: bool, pause: impl FnMut(), show: impl FnMut(&Entry), launch: impl FnOnce(&HostSpec) -> NorthResult<()>) -> NorthResult<()> {
        let record = self.record(name)?;
        if let Some(machine) = &record.machine {
            return Err(NorthError::Usage(format!("{name}'s conversation lives on {}; run the interactive UI there", machine.name)));
        }
        let conversation = record.conversation.clone().ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let request = LaunchRequest::relaunching(&record);
        let runtime = self.runtime(&record)?;
        let account = self.account(&request)?;
        runtime.prepare_interactive(&account, &request)?;
        self.refuse_beside_interactive(&record)?;
        let (instructions, _) = self.instructions(&record)?;
        let spec = runtime.interactive(&account, &request, &conversation, &instructions)?;
        if after_turn {
            self.await_turn_end(name, pause, show)?;
        }
        if !record.ended() {
            self.halt(name, Halt::Interactive)?;
        }
        Host::new(record.host_dir.clone()).await_end()?;
        launch(&spec)
    }

    /// Streams `name`'s running turn through `show` until it and any background task its CLI started have ended; on a host idle from the
    /// first look, waits `QUIET_LOOKS` looks first, so a message sent just after the host started begins its turn before the window takes it.
    fn await_turn_end(&self, name: &str, mut pause: impl FnMut(), mut show: impl FnMut(&Entry)) -> NorthResult<()> {
        let (mut follower, mut shown, mut quiet) = (None::<Follower>, 0, 0);
        loop {
            let working = self.work_running(name)?;
            if working && follower.is_none() {
                eprintln!("north fleet: {name} is mid-turn or runs a background task; its turn streams here and its window opens on the conversation when both have ended");
                follower = Some(self.follow(name)?);
            }
            if let Some(follower) = follower.as_mut() {
                follower.poll()?;
                let entries = &follower.transcript().entries;
                entries.iter().skip(shown).for_each(&mut show);
                shown = entries.len();
            }
            if !working && (follower.is_some() || quiet == QUIET_LOOKS) {
                return Ok(());
            }
            quiet += 1;
            pause();
        }
    }

    /// Completes the first-run answers north can write for `name`'s profile, then names any prompt its interactive UI would still stop at; `Err` when the profile cannot be written.
    pub fn interactive_prompt(&self, name: &str) -> NorthResult<Option<String>> {
        let (record, request) = (self.record(name)?, LaunchRequest::relaunching(&self.record(name)?));
        let runtime = self.runtime(&record)?;
        let account = self.account(&request)?;
        runtime.prepare_interactive(&account, &request)?;
        Ok(runtime.interactive_prompt(&account, &request))
    }

    fn work_running(&self, name: &str) -> NorthResult<bool> {
        let record = self.record(name)?;
        Ok(self.turn_running(name)? || record.live() && record.host_pid.is_some_and(|host| detached::cli_work(&self.proc, &self.cgroups, host) > 0))
    }

    fn turn_running(&self, name: &str) -> NorthResult<bool> {
        let record = self.record(name)?;
        Ok(session_running(&record) && (Unaccepted::outstanding(&record.host_dir) || self.recent(self.runtime(&record)?.as_ref(), &record)?.status == Status::Working))
    }

    pub(crate) fn refuse_beside_interactive(&self, record: &SessionRecord) -> NorthResult<()> {
        let Some(conversation) = &record.conversation else { return Ok(()) };
        match interactive_pid(&self.proc, conversation) {
            Some(pid) => Err(NorthError::Refused(format!("{} is open in its interactive UI (process {pid}); close that UI, then revive {} to bring it back into the fleet", record.name, record.name))),
            None => Ok(()),
        }
    }
}

/// A process running the provider CLI interactively on a conversation: it resumes the conversation and is not a `-p` stream host.
fn interactive_pid(proc: &Path, conversation: &str) -> Option<u32> {
    fs::read_dir(proc).ok()?.flatten().find_map(|entry| {
        let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
        let cmdline = fs::read(entry.path().join("cmdline")).ok()?;
        let words: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
        let resumes = words.windows(2).any(|pair| pair[0] == b"--resume" && pair[1] == conversation.as_bytes());
        (resumes && !words.iter().any(|word| *word == b"-p")).then_some(pid)
    })
}
