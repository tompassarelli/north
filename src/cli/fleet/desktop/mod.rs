//! The desktop a proxy held in a window is shown on: whenever the fleet starts it, a terminal window opens here running the provider's
//! interactive client on its conversation, so it runs where Tom types from its first turn. A proxy held by the screen opens nothing here: it
//! runs as a fleet host the fleet screen types into. The fleet asks through `ProxyDesk`; niri with ghostty is its adapter.
pub(super) mod niri;

use crate::fleet::registry::ProxyDesk;
use crate::fleet::runtime::HostSpec;

/// Tom's desktop: a ghostty window niri spawns, running the interactive client in its working directory and environment on the same HOME as
/// the fleet that opened it, so its hooks find that fleet; the client titles the window.
pub(super) struct Desktop;

impl ProxyDesk for Desktop {
    fn closed(&self) -> Option<String> {
        niri::live().err()
    }

    fn open(&self, name: &str, client: &HostSpec) -> Result<String, String> {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut command = vec!["env".to_owned(), "-C".to_owned(), client.cwd.display().to_string()];
        command.extend(client.env_remove.iter().flat_map(|key| ["-u".to_owned(), key.clone()]));
        command.push(format!("HOME={home}"));
        command.extend(client.env.iter().map(|(key, value)| format!("{key}={}", value.to_string_lossy())));
        command.extend(client.argv.iter().cloned());
        niri::open_terminal(&command).map(|window| format!("{name} opened in window {window}"))
    }
}
