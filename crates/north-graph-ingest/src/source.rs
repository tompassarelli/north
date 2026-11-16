use crate::facts::Sourced;
use crate::places::Places;
use crate::{coverage, crates, reads};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Coverage,
    Crates,
    Reads,
}

impl Source {
    pub const ALL: [Self; 3] = [Self::Coverage, Self::Crates, Self::Reads];

    pub fn name(self) -> &'static str {
        match self {
            Self::Coverage => "coverage",
            Self::Crates => "crates",
            Self::Reads => "reads",
        }
    }

    pub(crate) fn read(self, places: &Places) -> Result<Sourced, String> {
        match self {
            Self::Coverage => coverage::read(places).map(Sourced::from),
            Self::Crates => crates::read(places),
            Self::Reads => reads::read(places),
        }
    }
}
