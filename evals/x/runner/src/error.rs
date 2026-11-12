use std::fmt;

#[derive(Debug)]
pub enum RunError {
    Usage(String),
    Refused(String),
    Digest {
        image: String,
        recorded: String,
        found: String,
    },
    Failed(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

pub type Result<T> = std::result::Result<T, RunError>;

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "usage: {message}"),
            Self::Refused(message) => write!(f, "refused: {message}"),
            Self::Digest {
                image,
                recorded,
                found,
            } => write!(
                f,
                "image {image} has digest {found}, first pulled as {recorded}; the run stops"
            ),
            Self::Failed(message) => write!(f, "{message}"),
            Self::Io(error) => write!(f, "io: {error}"),
            Self::Json(error) => write!(f, "json: {error}"),
        }
    }
}

impl From<std::io::Error> for RunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RunError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
