//! What a box's check names that main no longer has: a `north` verb the CLI reference does not list
//! and a source path a landed commit deleted, which a worker would spend its first turns discovering.
use super::hotspots;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

static CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s(])north\s+([a-z][a-z0-9-]*)(?:\s+([a-z][a-z0-9-]*))?").expect("north call pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("word pattern"));

/// The words that follow one verb in the CLI reference.
#[derive(Clone, Debug, Default, PartialEq)]
struct Subverbs {
    words: BTreeSet<String>,
    nested: bool,
}

/// The verbs the CLI reference on origin/main lists, each with the words its lines go on with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verbs(BTreeMap<String, Subverbs>);

impl Verbs {
    pub fn parse(reference: &str) -> Self {
        let mut verbs: BTreeMap<String, Subverbs> = BTreeMap::new();
        for command in reference.lines().filter(|line| line.starts_with("  ") && !line.starts_with("   ")).filter_map(|line| line.trim_start().split("  ").next()) {
            let mut tokens = command.split_whitespace();
            let Some(verb) = tokens.next().filter(|verb| WORD.is_match(verb)) else { continue };
            let rest: Vec<&str> = tokens.collect();
            let entry = verbs.entry(verb.to_owned()).or_default();
            entry.nested |= rest.first().is_some_and(|word| WORD.is_match(word));
            entry.words.extend(rest.iter().map(|word| word.trim_matches(['[', ']', '(', ')', '|'])).filter(|word| WORD.is_match(word)).map(str::to_owned));
        }
        Self(verbs)
    }

    fn lacks(&self, verb: &str, sub: Option<&str>) -> Option<String> {
        match (self.0.get(verb), sub) {
            (None, _) => Some(format!("north {verb}")),
            (Some(known), Some(sub)) if known.nested && !known.words.contains(sub) => Some(format!("north {verb} {sub}")),
            _ => None,
        }
    }
}

/// The checks of origin/main, read once per run: its CLI reference and, per repository, the paths a landed commit deleted that its tree lacks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mainline {
    pub verbs: Option<Verbs>,
    pub gone: BTreeMap<String, BTreeSet<String>>,
}

impl Mainline {
    /// What the box `text` of an issue in `repo` names that main no longer has, for the hold's reason.
    pub fn retired(&self, repo: &str, text: &str) -> Option<String> {
        let verb = self.verbs.as_ref().and_then(|verbs| {
            text.split('`').skip(1).step_by(2).flat_map(|span| CALL.captures_iter(span)).find_map(|call| verbs.lacks(&call[1], call.get(2).map(|sub| sub.as_str())))
        });
        verb.or_else(|| hotspots::named(text).into_iter().find(|path| self.gone.get(repo).is_some_and(|gone| gone.contains(path))))
    }
}

/// The multi-segment paths `text` names that are not globs, which are the ones a deletion can retire.
pub fn candidates(text: &str) -> Vec<String> {
    hotspots::named(text).into_iter().filter(|path| path.contains('/') && !path.contains('*')).collect()
}
