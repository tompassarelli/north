use crate::facts::Sourced;
use crate::places::Places;
use crate::shas::Shas;
use crate::{corrections, coverage, crates, issues, ledger, losses, observations, pushes, ranking, reads, touches};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Mirror,
    Ranking,
    Ledger,
    Losses,
    Pushes,
    Touches,
    Corrections,
    Coverage,
    Crates,
    Reads,
    Observations,
}

impl Source {
    pub const ALL: [Self; 11] = [Self::Mirror, Self::Ranking, Self::Ledger, Self::Losses, Self::Pushes, Self::Touches, Self::Corrections, Self::Coverage, Self::Crates, Self::Reads, Self::Observations];

    pub fn name(self) -> &'static str {
        match self {
            Self::Mirror => "mirror",
            Self::Ranking => "ranking",
            Self::Ledger => "ledger",
            Self::Losses => "losses",
            Self::Pushes => "pushes",
            Self::Touches => "touches",
            Self::Corrections => "corrections",
            Self::Coverage => "coverage",
            Self::Crates => "crates",
            Self::Reads => "reads",
            Self::Observations => "observations",
        }
    }

    pub fn writers(self) -> &'static [&'static str] {
        match self {
            Self::Mirror => &[issues::WRITER, issues::RUNS],
            Self::Ranking => &[ranking::WRITER],
            Self::Ledger => &[ledger::CHECKER, ledger::LEDGER, ledger::THREADS],
            Self::Losses => &[losses::WRITER],
            Self::Pushes => &[pushes::WRITER],
            Self::Touches => &[touches::WRITER],
            Self::Corrections => &[corrections::WRITER],
            Self::Coverage => &[coverage::WRITER],
            Self::Crates => &[crates::WRITER],
            Self::Reads => &[reads::WRITER],
            Self::Observations => &[observations::LANDER, observations::GIT, ledger::CHECKER],
        }
    }

    pub(crate) fn read(self, places: &Places, shas: &Shas) -> Result<Sourced, String> {
        match self {
            Self::Mirror => issues::read(places, shas),
            Self::Ranking => ranking::read(places),
            Self::Ledger => ledger::read(places).map(Sourced::from),
            Self::Losses => losses::read(places),
            Self::Pushes => pushes::read(places).map(Sourced::from),
            Self::Touches => touches::read(places).map(Sourced::from),
            Self::Corrections => corrections::read(places).map(Sourced::from),
            Self::Coverage => coverage::read(places).map(Sourced::from),
            Self::Crates => crates::read(places),
            Self::Reads => reads::read(places),
            Self::Observations => unreachable!("ingest appends observations through observe"),
        }
    }
}
