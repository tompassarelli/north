use super::layout;
use super::probe::{self, Facts};

pub(super) fn verdict(facts: &Facts, problems: &[String], judged: bool) -> String {
    match (problems.is_empty(), probe::installing(facts), judged) {
        (false, _, _) => problems.join("; "),
        (true, true, _) => "installing: declared with no live runtime yet".to_owned(),
        (true, false, true) => "live and healthy".to_owned(),
        (true, false, false) => format!("healthy on {}; no landing is due yet, so whether it is live is not judged", facts.runtime.as_deref().map_or("no runtime", layout::short)),
    }
}
