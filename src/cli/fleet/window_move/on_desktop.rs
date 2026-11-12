//! The move's steps on this desktop: niri places the ghostty windows, the target profile receives the conversation and the fleet record
//! follows the window that holds it.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::PROXY;

use super::super::desktop::niri as desktop;
use super::session::{self, Window, arguments_of, environment_of, name_of, parent_of};
use super::steps::Mover;
use super::{ancestor_named, replacement};

const SETTLE: Duration = Duration::from_secs(90);

pub(super) struct OnDesktop<'a> {
    pub fleet: &'a Fleet,
    pub to: String,
    pub target: PathBuf,
    pub mark: u64,
}

pub(super) struct Opened {
    placed: desktop::Placed,
    client: u32,
    old: desktop::Placed,
    focused: Option<u64>,
    old_shell: Option<u32>,
}

impl Mover for OnDesktop<'_> {
    type Opened = Opened;

    fn carry(&self, window: &Window) -> NorthResult<()> {
        let copied = session::carry(window, &self.target)?;
        println!("north proxy move: carried {copied} session items into {}", self.target.display());
        Ok(())
    }

    fn open(&self, window: &Window) -> NorthResult<Opened> {
        let old_shell = parent_of(window.client).filter(|pid| name_of(*pid).ends_with("sh"));
        let terminal = ancestor_named(window.client, "ghostty").ok_or_else(|| NorthError::State("the window's terminal process is not ghostty".into()))?;
        let old = desktop::window_of_terminal(terminal)?;
        let focused = desktop::focused()?;
        let placed = desktop::open(&replacement(window, &self.to, terminal))?;
        match wait_for_client(window, &self.target, placed.pid) {
            Ok(client) => {
                println!("north proxy move: new window {} runs process {client} on {}", placed.id, self.target.display());
                Ok(Opened { placed, client, old, focused, old_shell })
            }
            Err(error) => {
                desktop::terminate(placed.pid);
                Err(error)
            }
        }
    }

    fn untouched(&self, window: &Window) -> bool {
        session::turn_ended(window) && session::activity(window) == self.mark
    }

    fn rebind(&self, window: &Window, opened: &Opened) -> NorthResult<()> {
        let record = self.fleet.adopt_moved(PROXY, &window.session.id, window.cwd.clone())?;
        let placement = format!("adopted window, process {}", opened.client);
        if record.account != self.to || record.placement.as_deref() != Some(placement.as_str()) {
            return Err(NorthError::State(format!("the fleet record names account {} and {}, not the new window on account {}", record.account, record.placement.unwrap_or_default(), self.to)));
        }
        Ok(())
    }

    fn deliver(&self) -> NorthResult<()> {
        let status = Command::new(std::env::current_exe()?).args(["fleet", "tick"]).status()?;
        if status.success() { Ok(()) } else { Err(NorthError::State(format!("the fleet tick that delivers the coordinator's messages exited {status}"))) }
    }

    fn end_old(&self, window: &Window, opened: &Opened) -> NorthResult<()> {
        stop_background_session(window);
        desktop::hang_up(opened.old_shell.unwrap_or(window.client));
        wait_gone(window.client);
        desktop::settle(&opened.placed, &opened.old, opened.focused)
    }

    fn close_new(&self, opened: Opened) {
        desktop::terminate(opened.placed.pid);
        wait_gone(opened.client);
    }

    fn restore(&self, window: &Window) -> NorthResult<()> {
        self.fleet.adopt_moved(PROXY, &window.session.id, window.cwd.clone()).map(drop)
    }
}

fn wait_for_client(window: &Window, target: &Path, terminal: u32) -> NorthResult<u32> {
    let deadline = Instant::now() + SETTLE;
    while Instant::now() < deadline {
        if let Some(client) = find_replacement(&window.session.id, terminal) {
            let record = target.join("sessions").join(format!("{client}.json"));
            let up = std::fs::read(&record).ok().and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok()).is_some_and(|record| record["status"] != "busy");
            if up && environment_of(client).get("CLAUDE_CONFIG_DIR").map(Path::new) == Some(target) {
                return Ok(client);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(NorthError::State(format!("the new window did not resume the conversation on {} within {} seconds; the old window is untouched", target.display(), SETTLE.as_secs())))
}

fn find_replacement(id: &str, terminal: u32) -> Option<u32> {
    std::fs::read_dir("/proc").ok()?.flatten().filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok()).find(|pid| {
        let arguments = arguments_of(*pid);
        arguments.first().is_some_and(|first| first.ends_with("/claude")) && arguments.iter().any(|argument| argument == id) && ancestor_named(*pid, "ghostty") == Some(terminal)
    })
}

fn stop_background_session(window: &Window) {
    let Some(job) = &window.session.job else { return };
    let Ok(runtime) = std::fs::read_link(format!("/proc/{}/exe", window.client)) else { return };
    let home = std::env::var("HOME").unwrap_or_default();
    let status = Command::new(runtime).env_clear().env("HOME", home).env("PATH", std::env::var("PATH").unwrap_or_default()).env("CLAUDE_CONFIG_DIR", &window.profile).args(["stop", job]).status();
    match status {
        Ok(status) if status.success() => println!("north proxy move: stopped background session {job} on account {}", window.account),
        other => eprintln!("north proxy move: stopping background session {job} did not succeed: {other:?}"),
    }
}

fn wait_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(250));
    }
    if Path::new(&format!("/proc/{pid}")).exists() {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}
