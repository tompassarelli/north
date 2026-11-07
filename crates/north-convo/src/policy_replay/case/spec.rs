use std::fs;
use std::path::Path;

use regex::Regex;
use toml_edit::DocumentMut;

use super::source::{self, Case, Named};
use crate::error::NorthError;
pub use crate::platform::replay_verdict::Kind;
use crate::threads::incidents::corrections;

pub enum Detector {
    Judge,
    Pattern { repeats: Option<Regex>, expects: Option<Regex> },
}

impl Detector {
    pub fn deterministic(&self) -> bool {
        matches!(self, Detector::Pattern { .. })
    }

    pub fn repeats(&self, lines: &[String]) -> Option<bool> {
        let Detector::Pattern { repeats, expects } = self else {
            return None;
        };
        let found = |pattern: &Regex| lines.iter().any(|line| pattern.is_match(line));
        Some(repeats.as_ref().is_some_and(found) || expects.as_ref().is_some_and(|pattern| !found(pattern)))
    }
}

pub struct Spec {
    pub kind: Kind,
    pub detector: Detector,
}

fn refusal(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay case: {message}"))
}

pub fn parse(argument: &str, text: &str) -> Result<(Spec, DocumentMut), NorthError> {
    let document: DocumentMut = text.parse().map_err(|error| refusal(format!("{argument}: {error}")))?;
    let word = document.get("kind").and_then(|item| item.as_str());
    let kind = word.and_then(Kind::parse).ok_or_else(|| refusal(format!("{argument}: kind is {}, not bad, guard or control", word.unwrap_or("missing"))))?;
    let pattern = |key: &str| {
        document.get("detector").and_then(|table| table.get(key)).and_then(|item| item.as_str()).map(Regex::new).transpose().map_err(|error| refusal(format!("{argument}: detector.{key}: {error}")))
    };
    let (repeats, expects) = (pattern("repeats")?, pattern("expects")?);
    let detector = if repeats.is_none() && expects.is_none() { Detector::Judge } else { Detector::Pattern { repeats, expects } };
    Ok((Spec { kind, detector }, document))
}

pub fn load(argument: &str) -> Result<(Case, Spec), NorthError> {
    if !argument.ends_with(".toml") {
        return Ok((source::resolve(argument)?, Spec { kind: Kind::Bad, detector: Detector::Judge }));
    }
    let path = Path::new(argument);
    let text = fs::read_to_string(path).map_err(|error| refusal(format!("{argument}: {error}")))?;
    let (spec, document) = parse(argument, &text)?;
    let field = |key: &str| document.get(key).and_then(|item| item.as_str()).map(str::to_owned);
    let id = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let mut case = match (field("correction"), field("session"), field("message")) {
        (Some(correction), None, None) => source::resolve(&correction)?,
        (None, Some(session), Some(message)) => {
            let class = field("class").unwrap_or_else(|| corrections::UNCLASSIFIED.to_owned());
            let paraphrase = field("behavior").unwrap_or_else(|| corrections::paraphrase(&class));
            source::resolve_prompt(Named { id: &id, class, paraphrase, session: &session }, &message)?
        }
        _ => return Err(refusal(format!("{argument}: name either correction, or session and message"))),
    };
    case.id = id;
    if let Some(behavior) = field("behavior") {
        case.paraphrase = behavior;
    }
    Ok((case, spec))
}
