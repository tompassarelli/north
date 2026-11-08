use super::finding::Finding;
use regex::Regex;
use std::sync::LazyLock;

static AGED_FROM_NOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:std\s*::\s*time\s*::\s*)?SystemTime\s*::\s*now\s*\(\s*\)(\s*-)").unwrap());

const FIXED_NOW: &str = "std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Fix {
    AgeFromFixedMoment,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Rewrite {
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

pub(super) fn apply(text: &str, findings: &[Finding]) -> (String, Vec<Rewrite>) {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let mut rewrites = Vec::new();
    for finding in findings {
        let Some(fix) = finding.fix else { continue };
        let Some(line) = finding.line.checked_sub(1).and_then(|index| lines.get_mut(index)) else { continue };
        let Some(after) = fix.apply(line) else { continue };
        rewrites.push(Rewrite { rule: finding.rule, path: finding.path.clone(), line: finding.line, before: line.trim().to_owned(), after: after.trim().to_owned() });
        *line = after;
    }
    (lines.join("\n"), rewrites)
}
