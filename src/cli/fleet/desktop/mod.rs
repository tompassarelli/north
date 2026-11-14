//! The desktop a proxy held in a window is shown on: whenever the fleet starts its host, its conversation opens in a terminal window here running
//! the interactive client, so it runs where Tom types. A proxy held by the screen opens nothing here: it runs as a fleet host the fleet screen
//! types into. The fleet asks through `ProxyDesk`; niri with ghostty is its adapter.
pub(super) mod niri;

use crate::fleet::registry::ProxyDesk;

/// Tom's desktop: a ghostty window niri spawns, running `north fleet interactive NAME --after-turn` on the same HOME as the fleet that
/// started the host, so the window finds that fleet and takes the conversation once the turn the host was woken for ends.
pub(super) struct Desktop;

impl ProxyDesk for Desktop {
    fn closed(&self) -> Option<String> {
        niri::live().err()
    }

    fn open(&self, name: &str) -> Result<String, String> {
        let north = std::env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string());
        let home = std::env::var("HOME").unwrap_or_default();
        let title = format!("north {name}");
        let command = ["env".to_owned(), format!("HOME={home}"), north, "fleet".to_owned(), "interactive".to_owned(), name.to_owned(), super::interactive::AFTER_TURN.to_owned()];
        niri::open_terminal(&title, &command).map(|window| format!("{name} opened in window {window} titled \"{title}\""))
    }
}
