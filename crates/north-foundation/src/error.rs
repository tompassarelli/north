use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NorthError {
    #[error("I/O failed: {0}")]
    Io(#[source] io::Error),
    #[error("I/O failed while {at}: {error}")]
    IoAt { at: String, #[source] error: io::Error },
    #[error("JSON failed: {0}")]
    Json(#[source] serde_json::Error),
    #[error("{0}")]
    Usage(String),
    #[error("North configuration failed: {0}")]
    Configuration(String),
    #[error("protocol failed: {0}")]
    Protocol(String),
    #[error("Incomplete North state: {0}")]
    State(String),
    #[error("{0}")]
    Rejected(String),
    #[error("gate {id} was already answered: {} by {by}", collapse_whitespace(.answer))]
    GateAnswered { id: i64, answer: String, by: String },
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Saturated(String),
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl NorthError {
    pub fn io_at(error: io::Error, at: impl FnOnce() -> String) -> Self {
        Self::IoAt { at: at(), error }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Saturated(_) => 75,
            _ => 1,
        }
    }

    pub fn user_message(&self) -> String {
        match self {
            Self::Protocol(message) if message.contains("401 Unauthorized") => {
                "Couldn’t connect: the provider rejected the authorization.".into()
            }
            Self::Protocol(message) if message.contains("turn ended as failed") => {
                "The provider couldn’t complete the request.".into()
            }
            Self::State(message) => format!("North couldn’t read the conversation state: {message}"),
            _ => self.to_string(),
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


pub type NorthResult<T> = Result<T, NorthError>;

#[cfg(test)]
#[path = "foundation/error_tests.rs"]
mod tests;
