//! The issue references a gate names and the boxes an issue body holds, read by the gate verbs.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

pub const OWNER: &str = "tompassarelli";

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com/([\w.-]+)/([\w.-]+)/issues/(\d+)|(?:([\w.-]+)/)?([A-Za-z][\w.-]*)?#(\d+)").expect("reference pattern")
});

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IssueRef {
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    pub fn new(repo: &str, number: u64) -> Self {
        Self { repo: repo.to_lowercase(), number }
    }

    /// `repo#N`, `owner/repo#N` or a GitHub issue URL.
    pub fn parse(text: &str) -> Option<Self> {
        references(text, "").into_iter().next().filter(|reference| !reference.repo.is_empty())
    }
}

impl fmt::Display for IssueRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

pub fn product_boxes(body: &str) -> usize {
    body.lines().filter(|line| UNTICKED.is_match(line)).count()
}

pub fn references(text: &str, repo: &str) -> Vec<IssueRef> {
    REFERENCE
        .captures_iter(text)
        .filter(|found| {
            let start = found.get(0).map_or(0, |m| m.start());
            !text[..start].ends_with(|c: char| c.is_alphanumeric() || c == '&')
        })
        .filter_map(|found| {
            let group = |index| found.get(index).map(|m| m.as_str());
            match (group(1), group(3)) {
                (Some(owner), Some(number)) if owner.eq_ignore_ascii_case(OWNER) => Some(IssueRef::new(group(2)?, number.parse().ok()?)),
                (Some(_), _) => None,
                _ => {
                    let owner = group(4);
                    if owner.is_some_and(|owner| !owner.eq_ignore_ascii_case(OWNER)) {
                        return None;
                    }
                    Some(IssueRef::new(group(5).unwrap_or(repo), group(6)?.parse().ok()?))
                }
            }
        })
        .collect()
}
