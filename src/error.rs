use std::error::Error;
use std::fmt;
use std::io;


#[derive(Debug)]
pub enum NorthError {
    Io(io::Error),
    Json(serde_json::Error),
    Usage(String),
    Configuration(String),
    Protocol(String),
    State(String),
    Rejected(String),
    GateAnswered { id: i64, answer: String, by: String },
    Refused(String),
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
            Self::State(message) => format!("North couldn’t read the conversation state: {message}"),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for NorthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Refused(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "JSON failed: {error}"),
            Self::Configuration(message) => {
                write!(formatter, "North configuration failed: {message}")
            }
            Self::Protocol(message) => write!(formatter, "Codex protocol failed: {message}"),
            Self::State(message) => write!(formatter, "Incomplete North state: {message}"),
            Self::GateAnswered { id, answer, by } => write!(formatter, "gate {id} was already answered: {} by {by}", answer.split_whitespace().collect::<Vec<_>>().join(" ")),
            Self::Rejected(message) => write!(formatter, "Codex rejected the request: {message}"),
        }
    }
}

impl Error for NorthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
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
