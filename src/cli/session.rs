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

impl Session {
    pub fn named(command: &str) -> Option<Self> {
        match command {
            "session-host" => Some(Self::Host),
            "session-link" => Some(Self::Link),
            _ => None,
        }
    }

    pub async fn run(self, arguments: &[String]) -> NorthResult<u8> {
        match self {
            Self::Host => session_host::run(arguments, fleet::host_watch).await,
            Self::Link => link::run(arguments).await,
        }
    }
}
