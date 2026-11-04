use std::fs;
use std::path::Path;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::model::SessionRecord;
use crate::fleet::runtime::HostSpec;

use super::Fleet;

impl Fleet {
    /// Hands a session to its provider's own interactive UI: stops the fleet host and waits until it has exited, so the fleet host and the UI never hold one conversation together, then passes the interactive command to `launch`. The record stays stopped, so nothing in the fleet restarts it; `revive` takes it back once the UI is closed.
    pub fn interactive(&self, name: &str, launch: impl FnOnce(&HostSpec) -> NorthResult<()>) -> NorthResult<()> {
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
        self.stop(name)?;
        Host::new(record.host_dir.clone()).await_end()?;
        launch(&spec)
    }

    pub(super) fn refuse_beside_interactive(&self, record: &SessionRecord) -> NorthResult<()> {
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
