//! Where the proxy runs, read from the surface on its record, which only a proxy-role record carries as held by the screen. A proxy held in a
//! window runs only in a window Tom types into and never on a fleet host: every start of it (a launch, a take-over, a revive, a relaunch, an
//! account move) opens a terminal on Tom's desktop running the provider's interactive client on its conversation, with its first prompt when it
//! has one, and binds the name to that conversation; it is refused where no such window can open. A proxy held by the screen runs as an
//! ordinary fleet host on this machine, typed into from the fleet screen, so only a conversation on another machine is refused and no window opens.
use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, LaunchRequest, Role, SessionRecord, Surface};
use crate::fleet::records::write_record;
use crate::fleet::remote::HooksCheck;
use crate::fleet::runtime::{HostSpec, Opening, Runtime};
use crate::fleet::store::Claim;

use super::Fleet;

/// The desktop Tom types on: the compositor and terminal sit behind it.
pub trait ProxyDesk: Send + Sync {
    /// Why no window can open on this desktop now, or None when one can.
    fn closed(&self) -> Option<String>;
    /// Opens a terminal window running `client`, the provider's interactive client on `name`'s conversation, and says what it opened.
    fn open(&self, name: &str, client: &HostSpec) -> Result<String, String>;
}

/// A fleet with no desktop attached, which therefore starts no proxy.
pub struct NoDesk;

impl ProxyDesk for NoDesk {
    fn closed(&self) -> Option<String> {
        Some("this fleet has no desktop attached".to_owned())
    }

    fn open(&self, _: &str, _: &HostSpec) -> Result<String, String> {
        Err("this fleet has no desktop attached".to_owned())
    }
}

/// A desk that opens every window it is asked for and keeps each name with the command its window runs, for fleets under test.
#[derive(Default)]
pub struct OpenedWindows(Mutex<Vec<(String, Vec<String>)>>);

impl OpenedWindows {
    pub fn opened(&self) -> Vec<(String, Vec<String>)> {
        self.0.lock().map(|opened| opened.clone()).unwrap_or_default()
    }
}

impl ProxyDesk for OpenedWindows {
    fn closed(&self) -> Option<String> {
        None
    }

    fn open(&self, name: &str, client: &HostSpec) -> Result<String, String> {
        self.0.lock().map_err(|_| "the window list is poisoned".to_owned())?.push((name.to_owned(), client.argv.clone()));
        Ok(format!("{name} opened in a window"))
    }
}

/// Whether `record` is a proxy held in a window, which no fleet host ever runs.
pub(super) fn held_in_window(record: &SessionRecord) -> bool {
    record.role() == Role::Proxy && record.surface == Surface::Window
}

/// Whether a launch starts its host beside its registration: a fresh one does, unless it is a proxy held in a window, which has no host.
pub(super) fn hosts_early(claim: Claim, record: &SessionRecord) -> bool {
    claim == Claim::Fresh && !held_in_window(record)
}

impl Fleet {
    pub fn hold_proxy_on(&self, name: &str, surface: Surface) -> NorthResult<()> {
        let _start = self.start_lock(name)?;
        let mut record = self.record(name)?;
        if record.surface != surface {
            record.surface = surface;
            write_record(&self.session_dir(name)?, &record)?;
        }
        Ok(())
    }

    /// Refuses to start a proxy that no window on Tom's desktop could take: one whose conversation lives on another machine, and for a proxy
    /// held in a window also one whose interactive client would stop at a first-run prompt, or on a host with no desktop.
    pub(super) fn refuse_headless_proxy(&self, record: &SessionRecord, runtime: &dyn Runtime, account: &Account) -> NorthResult<()> {
        if record.role() != Role::Proxy {
            return Ok(());
        }
        let why = match &record.machine {
            Some(machine) => Some(format!("its conversation lives on {}, away from Tom's desktop", machine.name)),
            None if record.surface == Surface::Screen => None,
            None => {
                let request = LaunchRequest::relaunching(record);
                runtime.prepare_interactive(account, &request)?;
                runtime.interactive_prompt(account, &request).map(|prompt| format!("its window would stop at a prompt: {prompt}")).or_else(|| self.desk.closed())
            }
        };
        match why {
            Some(why) => Err(NorthError::Refused(headless(&record.name, &why))),
            None => Ok(()),
        }
    }

    /// Starts a proxy held in a window: no fleet host, but a terminal on Tom's desktop running the provider's interactive client on
    /// `opening`'s conversation with `prompt` as its first turn, and once that window is up the name bound to the conversation.
    pub(super) fn open_proxy_window(&self, record: &mut SessionRecord, account: &Account, opening: Opening, prompt: Option<&str>, hooks: HooksCheck) -> NorthResult<()> {
        let (Opening::Fresh(Some(conversation)) | Opening::Resume(conversation)) = opening else {
            return Err(NorthError::State(format!("{} has no conversation id for its window to open", record.name)));
        };
        self.refuse_on_window(Some(conversation))?;
        let runtime = self.runtime(record)?;
        self.refuse_headless_proxy(record, runtime.as_ref(), account)?;
        self.refuse_reserve(record, account)?;
        if let Some(reason) = hooks.verdict()? {
            return Err(NorthError::Refused(reason));
        }
        let (instructions, sha) = self.instructions(record)?;
        let client = runtime.interactive(account, &LaunchRequest::relaunching(record), opening, prompt, &instructions)?;
        let line = self.desk.open(&record.name, &client).map_err(|why| NorthError::Refused(headless(&record.name, &format!("its window did not open: {why}"))))?;
        eprintln!("north fleet: {line}");
        record.conversation = Some(conversation.to_owned());
        record.system_prompt_sha = Some(sha);
        record.host_pid = None;
        self.hold_in_window(record)
    }
}

fn headless(name: &str, why: &str) -> String {
    format!("{name} is the proxy and runs only in a window Tom types into, so no headless host starts for it: {why}")
}

#[cfg(test)]
#[path = "proxy_window_tests.rs"]
mod tests;
