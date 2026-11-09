//! The pause the proxy records for north fleet dispatch on a host, under `~/.local/state/north`: while it exists dispatch
//! launches nothing, the timer is meant stopped, and its text says why.
pub const RECORD: &str = "dispatch-paused";
pub const TIMER: &str = "north-dispatch.timer";

/// Why dispatch is paused on the host whose home is `home`, while the proxy's pause stands.
pub fn standing(home: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(home.join(".local/state/north").join(RECORD)).ok().map(|reason| reason.trim().to_owned())
}
