//! The `north` verbs the CLI reference lists, and the `north` calls a box's backticked check names; dispatch and the landing scans read both the same way.
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

pub const REFERENCE: &str = "src/cli/reference.txt";

static CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s(])north\s+([a-z][a-z0-9-]*)(?:\s+([a-z][a-z0-9-]*))?").expect("north call pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("word pattern"));

#[derive(Clone, Debug, Default, PartialEq)]
struct Subverbs {
    words: BTreeSet<String>,
    nested: bool,
}

/// The verbs one CLI reference lists, each with the words its lines go on with.
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

    /// The call `north VERB [SUB]` as this reference lacks it, or `None` when the reference lists it.
    pub fn lacks(&self, call: &Call) -> Option<String> {
        match (self.0.get(&call.verb), call.sub.as_deref()) {
            (None, _) => Some(format!("north {}", call.verb)),
            (Some(known), Some(sub)) if known.nested && !known.words.contains(sub) => Some(format!("north {} {sub}", call.verb)),
            _ => None,
        }
    }
}

/// One `north VERB [SUB]` call a text names inside backticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub verb: String,
    pub sub: Option<String>,
}

pub fn calls(text: &str) -> Vec<Call> {
    text.split('`').skip(1).step_by(2).flat_map(|span| CALL.captures_iter(span)).map(|call| Call { verb: call[1].to_owned(), sub: call.get(2).map(|sub| sub.as_str().to_owned()) }).collect()
}
