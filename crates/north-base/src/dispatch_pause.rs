//! The pause the proxy records for north dispatch on a host, under `~/.local/state/north`: while it exists dispatch
//! launches nothing, the timer is meant stopped, and its text says why.
pub const RECORD: &str = "dispatch-paused";
pub const TIMER: &str = "north-dispatch.timer";
