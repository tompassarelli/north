//! Where the proxy runs, read from the surface on its record, which only a proxy-role record carries as held by the screen. A proxy held in a
//! window runs only in a window Tom types into: every start of its host (a launch, a take-over, a revive, a wake by a message, an account move)
//! is refused where no such window can hold it, and once the host is up the desk opens the window that takes its conversation as soon as the
//! turn it was woken for has ended, so no path leaves it headless. A proxy held by the screen runs as an ordinary fleet host on this machine,
//! typed into from the fleet screen, so only a conversation on another machine is refused and no window opens.
use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::model::{Account, LaunchRequest, Role, SessionRecord, Surface};
use crate::fleet::runtime::Runtime;

use super::Fleet;

/// The desktop Tom types on: the compositor and terminal sit behind it.
pub trait ProxyDesk: Send + Sync {
    /// Why no window can open on this desktop now, or None when one can.
    fn closed(&self) -> Option<String>;
    /// Opens a window running `name`'s interactive client once its host's turn ends, and says what it opened.
    fn open(&self, name: &str) -> Result<String, String>;
}

/// A fleet with no desktop attached, which therefore starts no proxy.
pub struct NoDesk;

impl ProxyDesk for NoDesk {
    fn closed(&self) -> Option<String> {
        Some("this fleet has no desktop attached".to_owned())
    }

    fn open(&self, _: &str) -> Result<String, String> {
        Err("this fleet has no desktop attached".to_owned())
    }
}

/// A desk that opens every window it is asked for and keeps their names, for fleets under test.
#[derive(Default)]
pub struct OpenedWindows(Mutex<Vec<String>>);

impl OpenedWindows {
    pub fn names(&self) -> Vec<String> {
        self.0.lock().map(|names| names.clone()).unwrap_or_default()
    }
}

impl ProxyDesk for OpenedWindows {
    fn closed(&self) -> Option<String> {
        None
    }

    fn open(&self, name: &str) -> Result<String, String> {
        self.0.lock().map_err(|_| "the window list is poisoned".to_owned())?.push(name.to_owned());
        Ok(format!("{name} opened in a window"))
    }
}

impl Fleet {
    /// Refuses to start a proxy host that no window on Tom's desktop could take over: one whose conversation lives on another machine, and for a
    /// proxy held in a window also one whose interactive client would stop at a first-run prompt, or on a host with no desktop.
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

    /// Opens the window that takes the conversation of a proxy held in a window from the host just started for it; a window that cannot open
    /// ends that host. A proxy held by the screen opens none.
    pub(super) fn show_proxy(&self, record: &SessionRecord, host: &Host) -> NorthResult<()> {
        if record.role() != Role::Proxy || record.surface == Surface::Screen {
            return Ok(());
        }
        match self.desk.open(&record.name) {
            Ok(line) => {
                eprintln!("north fleet: {line}");
                Ok(())
            }
            Err(why) => {
                host.terminate()?;
                Err(NorthError::Refused(headless(&record.name, &format!("its window did not open: {why}"))))
            }
        }
    }
}

fn headless(name: &str, why: &str) -> String {
    format!("{name} is the proxy and runs only in a window Tom types into, so no headless host starts for it: {why}")
}
