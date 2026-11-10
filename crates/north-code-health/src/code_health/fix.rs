use super::finding::{Finding, Fix};
use regex::Regex;
use std::sync::LazyLock;

static AGED_FROM_NOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:std\s*::\s*time\s*::\s*)?SystemTime\s*::\s*now\s*\(\s*\)(\s*-)").unwrap());

const FIXED_NOW: &str = "std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)";

#[derive(Debug, Eq, PartialEq)]
pub struct Suggestion {
    pub rule: &'static str,
    pub path: String,
    pub line: usize,
    pub before: String,
    pub after: String,
}

impl Fix {
    pub(super) fn declared_for(item: &str) -> Option<Self> {
        (item == "SystemTime::now").then_some(Self::AgeFromFixedMoment)
    }

    fn apply(self, line: &str) -> Option<String> {
        match self {
            Self::AgeFromFixedMoment => AGED_FROM_NOW.is_match(line).then(|| AGED_FROM_NOW.replace_all(line, format!("{FIXED_NOW}$1")).into_owned()),
        }
    }
}

pub(super) fn applies(finding: &Finding, text: &str) -> bool {
    let Some(fix) = finding.fix else { return false };
    finding.line.checked_sub(1).and_then(|index| text.split('\n').nth(index)).is_some_and(|line| fix.apply(line).is_some())
}

pub(super) fn suggest(text: &str, findings: &[Finding]) -> Vec<Suggestion> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut suggestions = Vec::new();
    for finding in findings {
        let Some(fix) = finding.fix else { continue };
        let Some(before) = finding.line.checked_sub(1).and_then(|index| lines.get(index)) else { continue };
        let Some(after) = fix.apply(before) else { continue };
        suggestions.push(Suggestion { rule: finding.rule, path: finding.path.clone(), line: finding.line, before: before.trim().to_owned(), after: after.trim().to_owned() });
    }
    suggestions
}
