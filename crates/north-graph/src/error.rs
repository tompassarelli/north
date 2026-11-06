use std::fmt;

use crate::term::TermError;

#[derive(Debug)]
pub enum GraphError {
    Invalid(String),
    Storage(rusqlite::Error),
    Corrupt(String),
    Query(QueryError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    Invalid(String),
    StepBudgetExceeded { steps: u64 },
    Timeout { milliseconds: u64 },
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => write!(formatter, "invalid graph input: {why}"),
            Self::Storage(why) => write!(formatter, "graph storage: {why}"),
            Self::Corrupt(why) => write!(formatter, "graph store is corrupt: {why}"),
            Self::Query(why) => why.fmt(formatter),
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => write!(formatter, "invalid query: {why}"),
            Self::StepBudgetExceeded { steps } => write!(formatter, "query exceeded its step budget of {steps}"),
            Self::Timeout { milliseconds } => write!(formatter, "query exceeded its timeout of {milliseconds} ms"),
        }
    }
}

impl std::error::Error for GraphError {}
impl std::error::Error for QueryError {}

impl From<rusqlite::Error> for GraphError {
    fn from(why: rusqlite::Error) -> Self {
        Self::Storage(why)
    }
}

impl From<QueryError> for GraphError {
    fn from(why: QueryError) -> Self {
        Self::Query(why)
    }
}

impl From<TermError> for GraphError {
    fn from(why: TermError) -> Self {
        Self::Corrupt(why.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GraphError>;
