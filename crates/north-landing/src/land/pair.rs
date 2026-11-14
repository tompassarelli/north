//! One check read twice at a base tree and twice at a candidate tree by the same north in one slot, and what the pair says. Only a
//! candidate that fails both its runs the same way over a base that passes both regressed, and only a regression may block or revert
//! it. A tree whose two runs answer differently names the check as nondeterministic and blames no commit, a run that gives no
//! verdict leaves the pair without one, and a candidate red another way than a red base is a red that changed, which blames nothing.
use std::path::Path;

/// One run of a check on one tree: green, red with the signature two runs must share to fail the same way, or no verdict with what the check said.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    Green(String),
    Red(String),
    NoVerdict(String),
}

impl Reading {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Green(_), Self::Green(_)) => true,
            (Self::Red(one), Self::Red(other)) => one == other,
            _ => false,
        }
    }

    pub fn word(&self) -> String {
        let first = |said: &str| said.lines().next().unwrap_or_default().to_owned();
        match self {
            Self::Green(said) => format!("green ({})", first(said)),
            Self::Red(said) => format!("red ({})", first(said)),
            Self::NoVerdict(said) => format!("no verdict ({})", first(said)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pair {
    Regressed,
    Fixed,
    SameRed,
    RedChanged,
    SameGreen,
    NoVerdict,
    TwoAnswers,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compared {
    pub pair: Pair,
    pub base: [Reading; 2],
    pub candidate: [Reading; 2],
}

impl Compared {
    pub fn regressed(&self) -> bool {
        self.pair == Pair::Regressed
    }
}

/// Reads `check` at `base`, `candidate`, `base` and `candidate` in turn in this process, so one north and one slot give all four
/// answers and a change on the host between runs shows as two answers in one tree, never as a regression.
pub fn compare(check: &dyn Fn(&Path) -> Reading, base: &Path, candidate: &Path) -> Compared {
    let (base_first, candidate_first) = (check(base), check(candidate));
    let (base_second, candidate_second) = (check(base), check(candidate));
    let (base, candidate) = ([base_first, base_second], [candidate_first, candidate_second]);
    Compared { pair: judge(&base, &candidate), base, candidate }
}

fn judge(base: &[Reading; 2], candidate: &[Reading; 2]) -> Pair {
    if base.iter().chain(candidate).any(|reading| matches!(reading, Reading::NoVerdict(_))) {
        return Pair::NoVerdict;
    }
    let (Some(before), Some(after)) = (answer(base), answer(candidate)) else { return Pair::TwoAnswers };
    match (before, after) {
        (Reading::Green(_), Reading::Red(_)) => Pair::Regressed,
        (Reading::Red(_), Reading::Green(_)) => Pair::Fixed,
        (Reading::Red(one), Reading::Red(other)) if one == other => Pair::SameRed,
        (Reading::Red(_), Reading::Red(_)) => Pair::RedChanged,
        _ => Pair::SameGreen,
    }
}

fn answer(runs: &[Reading; 2]) -> Option<&Reading> {
    runs[0].same(&runs[1]).then_some(&runs[0])
}

#[cfg(test)]
#[path = "pair_tests.rs"]
mod tests;
