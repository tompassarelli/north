use std::fmt;
use std::io;

pub(super) enum Failure {
    Unreconciled(String),
    Unsafe(String),
    Other(String),
    Pipe,
}

pub(super) type Result<T> = std::result::Result<T, Failure>;

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::BrokenPipe { Self::Pipe } else { Self::Other(error.to_string()) }
    }
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Other(error.to_string())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreconciled(message) => write!(formatter, "refresh inconclusive: {message}"),
            Self::Unsafe(message) => write!(formatter, "refusing to sweep: {message}"),
            Self::Other(message) => formatter.write_str(message),
            Self::Pipe => Ok(()),
        }
    }
}

impl Failure {
    pub(super) fn exit_code(&self) -> u8 {
        match self {
            Self::Unreconciled(_) => 2,
            Self::Unsafe(_) => 3,
            Self::Other(_) => 1,
            Self::Pipe => 0,
        }
    }
}
