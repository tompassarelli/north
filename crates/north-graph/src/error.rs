use crate::term::TermError;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("invalid graph input: {0}")]
    Invalid(String),
    #[error("graph storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("graph store is corrupt: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Query(#[from] QueryError),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum QueryError {
    #[error("invalid query: {0}")]
    Invalid(String),
    #[error("query exceeded its step budget of {steps}")]
    StepBudgetExceeded { steps: u64 },
    #[error("query exceeded its timeout of {milliseconds} ms")]
    Timeout { milliseconds: u64 },
    #[error("invalid query snapshot: {0}")]
    InvalidSnapshot(String),
    #[error(
        "query snapshot expired: bound {bound} is below the retention horizon {horizon}, whose history is gone"
    )]
    SnapshotExpired { bound: i64, horizon: i64 },
    #[error(
        "text relation {relation} reads the live text index, which cannot answer asOf or since"
    )]
    TextUnderView { relation: String },
}

impl From<TermError> for GraphError {
    fn from(why: TermError) -> Self {
        Self::Corrupt(why.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GraphError>;
