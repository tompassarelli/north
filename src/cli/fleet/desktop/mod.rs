//! The desktop a fleet session is shown on: when a take-over or hand-over gives the proxy role to a new session, its conversation opens in a
//! terminal window here, so Tom sees the new proxy at once. The compositor and terminal sit behind `Opener`; niri with ghostty is its adapter.
pub(super) mod niri;

use crate::fleet::model::{Role, SessionRecord};

pub(super) trait Opener {
    /// Opens `command` in a new terminal window titled `title` and returns the window's id, or says why no window opened.
    fn open(&self, title: &str, command: &[String]) -> Result<u64, String>;
}

/// Opens the proxy a take-over or hand-over just started in its own window running `north fleet interactive --after-turn`, so the proxy's first turn ends before the window takes the conversation, on the same HOME as the launch so the window finds the fleet that started it, and says what it did; None for any other role.
pub(super) fn show_proxy(opener: &dyn Opener, record: &SessionRecord, north: &str, home: &str) -> Option<String> {
    if record.role() != Role::Proxy {
        return None;
    }
    let name = &record.name;
    if let Some(machine) = &record.machine {
        return Some(format!("{name} runs on {}, so no window opened on this desktop", machine.name));
    }
    let title = format!("north {name}");
    let command = ["env".to_owned(), format!("HOME={home}"), north.to_owned(), "fleet".to_owned(), "interactive".to_owned(), name.clone(), super::interactive::AFTER_TURN.to_owned()];
    Some(match opener.open(&title, &command) {
        Ok(window) => format!("{name} opened in window {window} titled \"{title}\", resuming conversation {}", record.conversation.as_deref().unwrap_or("none")),
        Err(why) => format!("{name} runs headless and no window opened: {why}"),
    })
}

/// Shows a session that just took a name over on this host's desktop when it is the proxy, and notes the outcome on stderr.
pub(super) fn show_taken(record: &SessionRecord) {
    let north = std::env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string());
    let home = std::env::var("HOME").unwrap_or_default();
    if let Some(line) = show_proxy(&niri::NiriGhostty, record, &north, &home) {
        eprintln!("north fleet: {line}");
    }
}
