//! What a box's check names that main no longer has: a `north` verb the CLI reference does not list
//! and a source path a landed commit deleted, which a worker would spend its first turns discovering.
use super::hotspots;
use super::issues::{Issue, IssueRef};
use north_cli_reference::Verbs;
use std::collections::{BTreeMap, BTreeSet};

/// The checks of origin/main, read once per run: its CLI reference and, per repository, the paths a landed commit deleted that its tree lacks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mainline {
    pub verbs: Option<Verbs>,
    pub gone: BTreeMap<String, BTreeSet<String>>,
}

impl Mainline {
    /// The boxes of `issue` whose check main still has; each other box is skipped with what it names that main lost.
    pub fn unretired(&self, issue: &Issue, boxes: Vec<Option<String>>, skipped: &mut Vec<(IssueRef, String)>) -> Vec<Option<String>> {
        boxes
            .into_iter()
            .filter(|text| match text.as_deref().and_then(|text| Some((text, self.retired(&issue.repo, text)?))) {
                Some((text, what)) => {
                    skipped.push((issue.reference(), format!("ready, not placed: box `{text}` check retired: {what}")));
                    false
                }
                None => true,
            })
            .collect()
    }

    /// What the box `text` of an issue in `repo` names that main no longer has, for the hold's reason.
    fn retired(&self, repo: &str, text: &str) -> Option<String> {
        let verb = self.verbs.as_ref().and_then(|verbs| north_cli_reference::calls(text).iter().find_map(|call| verbs.lacks(call)));
        verb.or_else(|| hotspots::named(text).into_iter().find(|path| self.gone.get(repo).is_some_and(|gone| gone.contains(path))))
    }
}

/// The multi-segment paths `text` names that are not globs, which are the ones a deletion can retire.
pub fn candidates(text: &str) -> Vec<String> {
    hotspots::named(text).into_iter().filter(|path| path.contains('/') && !path.contains('*')).collect()
}
