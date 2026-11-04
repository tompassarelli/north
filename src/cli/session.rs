//! The two processes behind a fleet session: the host that owns its stream and socket, and the link
//! that carries a session placed on another machine.
use crate::error::NorthResult;
use crate::fleet::{link, session_host};

use super::fleet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Session {
    Host,
    Link,
}

pub const NAMES: &[(&str, Session)] = &[("session-host", Session::Host), ("session-link", Session::Link)];

impl Session {
    pub fn named(command: &str) -> Option<Self> {
        NAMES.iter().find(|(name, _)| *name == command).map(|(_, session)| *session)
    }

    pub async fn run(self, arguments: &[String]) -> NorthResult<u8> {
        match self {
            Self::Host => session_host::run(arguments, fleet::host_watch).await,
            Self::Link => link::run(arguments).await,
        }
    }
}
