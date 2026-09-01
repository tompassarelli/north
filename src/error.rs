use std::error::Error;
use std::fmt;
use std::io;
use std::process::ExitStatus;

use clause_workbench::ResidentSourceWorkbenchErrorV1;

#[derive(Debug)]
pub enum NorthError {
    Io(io::Error),
    Json(serde_json::Error),
    Clause(ResidentSourceWorkbenchErrorV1),
    Configuration(String),
    Protocol(String),
    Interrupted,
    AppServerExit(ExitStatus),
}

impl NorthError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Protocol(message) if message.contains("401 Unauthorized") => {
                "Couldn’t connect to Codex: authorization was rejected.".into()
            }
            Self::Protocol(message) if message.contains("turn ended as failed") => {
                "Codex couldn’t complete the request.".into()
            }
            Self::Interrupted => "Interrupted".into(),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for NorthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "JSON failed: {error}"),
            Self::Clause(error) => write!(formatter, "Conversation state failed: {error}"),
            Self::Configuration(message) => {
                write!(formatter, "North configuration failed: {message}")
            }
            Self::Protocol(message) => write!(formatter, "Codex protocol failed: {message}"),
            Self::Interrupted => write!(formatter, "Codex turn was interrupted"),
            Self::AppServerExit(status) => {
                write!(formatter, "Codex app-server exited unexpectedly: {status}")
            }
        }
    }
}

impl Error for NorthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Clause(error) => Some(error),
            Self::Configuration(_)
            | Self::Protocol(_)
            | Self::Interrupted
            | Self::AppServerExit(_) => None,
        }
    }
}

impl From<io::Error> for NorthError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for NorthError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<ResidentSourceWorkbenchErrorV1> for NorthError {
    fn from(error: ResidentSourceWorkbenchErrorV1) -> Self {
        Self::Clause(error)
    }
}

pub type NorthResult<T> = Result<T, NorthError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_authorization_failures_are_safe_for_the_tui() {
        let error = NorthError::Protocol(
            "turn ended as failed: raw payload; HTTP error: 401 Unauthorized".into(),
        );

        assert_eq!(
            error.user_message(),
            "Couldn’t connect to Codex: authorization was rejected."
        );
        assert!(!error.user_message().contains("raw payload"));
    }

    #[test]
    fn interruption_has_plain_user_facing_copy() {
        assert_eq!(NorthError::Interrupted.user_message(), "Interrupted");
    }
}
