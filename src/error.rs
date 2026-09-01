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
    AppServerExit(ExitStatus),
}

impl fmt::Display for NorthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "JSON failed: {error}"),
            Self::Clause(error) => write!(formatter, "Clause transition failed: {error}"),
            Self::Configuration(message) => {
                write!(formatter, "North configuration failed: {message}")
            }
            Self::Protocol(message) => write!(formatter, "Codex protocol failed: {message}"),
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
            Self::Configuration(_) | Self::Protocol(_) | Self::AppServerExit(_) => None,
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
