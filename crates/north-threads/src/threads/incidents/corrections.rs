//! The classes of Tom's corrections declared in harness/corrections.toml: the rule that names each,
//! its neutral paraphrase, the issue that owns its guard, and the recurrences of a guarded class.
use std::sync::OnceLock;

use regex::Regex;
use toml_edit::DocumentMut;

use crate::foundation::clock::Timestamp;

pub const TABLE: &str = "harness/corrections.toml";
pub const UNCLASSIFIED: &str = "unclassified";
const UNCLASSIFIED_PARAPHRASE: &str = "Tom corrected the reply; no rule names its class yet.";
const RECURRENCE_WINDOW_S: f64 = 7.0 * 24.0 * 3600.0;

#[derive(Debug)]
pub struct Class {
    pub name: String,
    pub pattern: Option<Regex>,
    pub paraphrase: String,
    pub owner: Option<String>,
    pub skill: Option<String>,
    pub guard: Option<String>,
    pub guarded_at: Option<f64>,
    pub structure: Structure,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Structure {
    GraphGuard(String),
    Behavioural(String),
}

impl Class {
    pub fn graph_guard(&self) -> Option<&str> {
        match &self.structure {
            Structure::GraphGuard(name) => Some(name),
            Structure::Behavioural(_) => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Vec<Class>, String> {
    let doc: DocumentMut = text.parse().map_err(|error| format!("{TABLE}: {error}"))?;
    let tables = doc.get("class").and_then(|item| item.as_array_of_tables()).ok_or_else(|| format!("{TABLE}: no [[class]] tables"))?;
    tables
        .iter()
        .map(|table| {
            let text = |key: &str| table.get(key).and_then(|item| item.as_str()).map(str::to_owned);
            let name = text("name").ok_or_else(|| format!("{TABLE}: a class has no name"))?;
            let paraphrase = text("paraphrase").ok_or_else(|| format!("{TABLE}: class {name} has no paraphrase"))?;
            let pattern = text("pattern").map(|body| Regex::new(&format!(r"(?i)(?-u:\b)({body})")).map_err(|error| format!("{TABLE}: class {name}: {error}"))).transpose()?;
            let guarded_at = table
                .get("guarded_at")
                .and_then(|item| item.as_str().map(str::to_owned).or_else(|| item.as_datetime().map(ToString::to_string)))
                .map(|at| Timestamp::parse_loose(&at).map(|t| t.unix_seconds_f64()).ok_or_else(|| format!("{TABLE}: class {name}: guarded_at {at} is not a time")))
                .transpose()?;
            let structure = match (text("graph_guard"), text("not_structural")) {
                (Some(guard), None) => Structure::GraphGuard(guard),
                (None, Some(why)) => Structure::Behavioural(why),
                _ => return Err(format!("{TABLE}: class {name} needs exactly one of graph_guard (its harness/graph-guards file) and not_structural (why no graph fact holds it)")),
            };
            Ok(Class { name, pattern, paraphrase, owner: text("owner"), skill: text("skill"), guard: text("guard"), guarded_at, structure })
        })
        .collect()
}

pub fn classes() -> &'static [Class] {
    static SET: OnceLock<Vec<Class>> = OnceLock::new();
    SET.get_or_init(|| parse(north_base::platform::harness::table(TABLE)).unwrap_or_else(|error| panic!("{error}")))
}

pub fn class_of(text: &str) -> Option<&'static Class> {
    classes().iter().find(|class| class.pattern.as_ref().is_some_and(|pattern| pattern.is_match(text)))
}

pub fn named(name: &str) -> Option<&'static Class> {
    classes().iter().find(|class| class.name == name)
}

pub fn paraphrase(class: &str) -> String {
    named(class).map_or(UNCLASSIFIED_PARAPHRASE, |class| class.paraphrase.as_str()).to_owned()
}

#[derive(Debug, PartialEq)]
pub struct Recurrence {
    pub class: String,
    pub count: usize,
    pub owner: Option<String>,
}

pub fn recurrences(seen: &[(String, f64)], classes: &[Class], now: f64) -> Vec<Recurrence> {
    classes
        .iter()
        .filter_map(|class| {
            let guarded = class.guarded_at?;
            let count = seen.iter().filter(|(name, at)| *name == class.name && *at > guarded && now - at <= RECURRENCE_WINDOW_S).count();
            (count > 0).then(|| Recurrence { class: class.name.clone(), count, owner: class.owner.clone() })
        })
        .collect()
}

pub fn red(recurred: &[Recurrence]) -> (String, String) {
    if recurred.is_empty() {
        return (String::new(), String::new());
    }
    let set = recurred.iter().map(|r| format!("{}:{}", r.class, r.count)).collect::<Vec<_>>().join(";");
    let lines = recurred
        .iter()
        .map(|r| format!("{} recurred {} time{} since its guard landed (owner {})", r.class, r.count, if r.count == 1 { "" } else { "s" }, r.owner.as_deref().unwrap_or("none")))
        .collect::<Vec<_>>();
    (set, format!("corrections red ({}): {}",recurred.len(), lines.join("; ")))
}

#[cfg(test)]
#[path = "corrections_tests.rs"]
mod tests;
