use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::model::{Entry, SessionRecord, Status};
use crate::fleet::runtime::HostSpec;

use super::detached;
use super::{Fleet, session_running};
use super::stopping::Halt;

const TURN_POLL: Duration = Duration::from_millis(500);

impl Fleet {
    /// Hands a session to its provider's own interactive UI: with `after_turn`, first shows the turn the host is running through `show` as it streams and waits until it has ended and no background task its CLI started still runs, instead of cutting either off (stopping the host kills them); stops the fleet host and waits until it has exited, so the fleet host and the UI never hold one conversation together, then passes the interactive command to `launch`. The record stays stopped, so nothing in the fleet restarts it; `revive` takes it back once the UI is closed.
    pub fn interactive(&self, name: &str, after_turn: bool, show: impl FnMut(&Entry), launch: impl FnOnce(&HostSpec) -> NorthResult<()>) -> NorthResult<()> {
        self.interactive_pacing(name, after_turn, || sleep(TURN_POLL), show, launch)
    }

    /// `interactive` with the pause between two looks at the running turn handed in, so a caller that owns the turn can end it between the looks.
    pub fn interactive_pacing(&self, name: &str, after_turn: bool, mut pause: impl FnMut(), mut show: impl FnMut(&Entry), launch: impl FnOnce(&HostSpec) -> NorthResult<()>) -> NorthResult<()> {
        let record = self.record(name)?;
        if let Some(machine) = &record.machine {
            return Err(NorthError::Usage(format!("{name}'s conversation lives on {}; run the interactive UI there", machine.name)));
        }
        let conversation = record.conversation.clone().ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let runtime = self.runtime(&record)?;
        let account = self.account(&record.relaunch_request())?;
        self.refuse_beside_interactive(&record)?;
        let (instructions, _) = self.instructions(&record)?;
        let spec = runtime.interactive(&account, &record.relaunch_request(), &conversation, &instructions)?;
        if after_turn && self.work_running(name)? {
            eprintln!("north fleet: {name} is mid-turn or runs a background task; its turn streams here and its window opens on the conversation when both have ended");
            let mut follower = self.follow(name)?;
            let mut shown = 0;
            loop {
                follower.poll()?;
                let entries = &follower.transcript().entries;
                entries.iter().skip(shown).for_each(&mut show);
                shown = entries.len();
                if !self.work_running(name)? {
                    break;
                }
                pause();
            }
        }
        self.halt(name, Halt::Interactive)?;
        Host::new(record.host_dir.clone()).await_end()?;
        launch(&spec)
    }

    pub fn interactive_prompt(&self, name: &str) -> NorthResult<Option<String>> {
        let (record, request) = (self.record(name)?, self.record(name)?.relaunch_request());
        Ok(self.runtime(&record)?.interactive_prompt(&self.account(&request)?, &request))
    }

    fn work_running(&self, name: &str) -> NorthResult<bool> {
        let record = self.record(name)?;
        Ok(self.turn_running(name)? || record.live() && record.host_pid.is_some_and(|host| detached::cli_work(&self.proc, &self.cgroups, host) > 0))
    }

    fn turn_running(&self, name: &str) -> NorthResult<bool> {
        let record = self.record(name)?;
        Ok(session_running(&record) && self.recent(self.runtime(&record)?.as_ref(), &record)?.status == Status::Working)
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
