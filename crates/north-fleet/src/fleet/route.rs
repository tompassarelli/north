//! A two-rung route's words (north#1233): the `--route` a launch names, its rungs of model family and
//! optional effort, and the note the first rung's brief ends on. `rungs` escalates from the first rung to the second.
use super::effort::Effort;

pub const SMALL_FIRST: &str = "small-first";
pub const AFTER: &str = "after:";
const FAMILIES: [&str; 3] = ["haiku", "sonnet", "opus"];
pub const NOTE: &str = "\n\nTwo-rung route: you run on the first rung of a two-rung route. When your finish call's check fails, or a check the repository runs before queueing refuses the lane it committed, do not retry: end your turn at once with `Not done:` and `Stop: reasoning`, and when you cannot finish, still make the finish call with the box's check; either way the box starts once on the second rung with your diff and the check's output.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rung {
    pub family: &'static str,
    pub effort: Option<Effort>,
}

impl Rung {
    fn parse(text: &str) -> Option<Self> {
        let (family, effort) = match text.split_once(':') {
            Some((family, effort)) => (family, Some(Effort::parse(effort).ok()?)),
            None => (text, None),
        };
        Some(Self { family: FAMILIES.into_iter().find(|known| *known == family)?, effort })
    }

    pub fn word(&self) -> String {
        self.effort.map_or_else(|| self.family.to_owned(), |effort| format!("{}:{}", self.family, effort.label()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    First(Rung, Rung),
    Second { after: String, rung: Rung },
}

impl Route {
    pub fn parse(text: &str) -> Option<Self> {
        if text == SMALL_FIRST {
            return Some(Self::First(Rung { family: "haiku", effort: None }, Rung { family: "opus", effort: None }));
        }
        if let Some(rest) = text.strip_prefix(AFTER) {
            let (after, rung) = rest.split_once(':').filter(|(after, _)| !after.is_empty())?;
            return Some(Self::Second { after: after.to_owned(), rung: Rung::parse(rung)? });
        }
        let (first, second) = text.split_once(',')?;
        Some(Self::First(Rung::parse(first)?, Rung::parse(second)?))
    }

    pub fn rung(&self) -> &Rung {
        match self {
            Self::First(first, _) => first,
            Self::Second { rung, .. } => rung,
        }
    }
}

pub fn first(route: Option<&str>) -> bool {
    matches!(route.and_then(Route::parse), Some(Route::First(..)))
}
