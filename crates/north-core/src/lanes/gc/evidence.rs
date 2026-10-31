use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::exec::{command, git};

const CHERRY_LIMIT: u64 = 400;
static ISSUES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:Refs|Fixes|Closes|Part of)\s+([\w.-]*#\d+)").unwrap());
static REFS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bRefs\s+([\w.-]+#\d+)").unwrap());
static ORIGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)tompassarelli/([^/]+?)(?:\.git)?$").unwrap()
});

pub(super) struct Landed {
    pub(super) landed: bool,
    pub(super) ahead: u64,
    pub(super) how: String,
}

fn count(repo: &Path, range: &str) -> u64 {
    git(repo, &["rev-list", "--count", range]).out.parse().unwrap_or(0)
}

pub(super) fn landed(repo: &Path, base: &str, tip: &str) -> Landed {
    let yes = |ahead, how: &str| Landed { landed: true, ahead, how: how.to_string() };
    if git(repo, &["merge-base", "--is-ancestor", tip, base]).ok {
        return yes(0, "ancestor");
    }
    let ahead = count(repo, &format!("{base}..{tip}"));
    let tree = command(&["north", "landed-tree", "--repo", &repo.to_string_lossy(), base, tip]);
    if tree.ok {
        return yes(ahead, &tree.out);
    }
    let merged = git(repo, &["merge-tree", "--write-tree", base, tip]);
    if merged.ok && merged.out.lines().next() == Some(git(repo, &["rev-parse", &format!("{base}^{{tree}}")]).out.as_str()) {
        return yes(ahead, "adds nothing");
    }
    if count(repo, &format!("{tip}..{base}")) <= CHERRY_LIMIT {
        let cherry = git(repo, &["cherry", base, tip]);
        if cherry.ok && !cherry.out.lines().any(|line| line.starts_with('+')) {
            return yes(ahead, "patch-equivalent");
        }
    }
    Landed { landed: false, ahead, how: String::new() }
}

pub(super) fn item_ref(item: &str) -> String {
    item.strip_prefix("nixos-config#").map_or_else(|| item.to_string(), |rest| format!("firn#{rest}"))
}

pub(super) fn protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master" | "gh-pages") || branch.starts_with("release/")
}

pub(super) fn origin_repo(main: &Path) -> Option<String> {
    let url = git(main, &["remote", "get-url", "origin"]);
    if !url.ok {
        return None;
    }
    ORIGIN.captures(&url.out).map(|found| found[1].to_string())
}

fn unique(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}

fn bodies(main: &Path, base: &str, tip: &str, unavailable: &str) -> Result<String, String> {
    let log = git(main, &["log", "--format=%B", &format!("{base}..{tip}")]);
    if log.ok { Ok(log.out) } else { Err(unavailable.to_string()) }
}

pub(super) fn refs_on(main: &Path, base: &str, tip: &str) -> Result<Vec<String>, String> {
    let text = bodies(main, base, tip, "branch claim evidence unavailable")?;
    Ok(unique(REFS.captures_iter(&text).map(|found| item_ref(&found[1]))))
}

pub(super) fn named_issues(main: &Path, name: &str, base: &str, tip: &str) -> Result<Vec<String>, String> {
    let text = bodies(main, base, tip, "issue evidence unavailable")?;
    let own = origin_repo(main).unwrap_or_else(|| name.to_string());
    let qualified = |item: &str| if item.starts_with('#') { format!("{own}{item}") } else { item.to_string() };
    Ok(unique(ISSUES.captures_iter(&text).map(|found| item_ref(&qualified(&found[1])))))
}

pub(super) fn cited_issues(text: &str) -> Vec<String> {
    unique(ISSUES.captures_iter(text).map(|found| found[1].to_string()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IssueState {
    Open,
    Closed,
    Unknown,
}

pub(super) fn issue_state(item: &str) -> IssueState {
    let Some((repo, number)) = item.split_once('#') else {
        return IssueState::Unknown;
    };
    let ran = command(&["gh", "issue", "view", number, "-R", &format!("tompassarelli/{repo}"), "--json", "state", "-q", ".state"]);
    match (ran.ok, ran.out.as_str()) {
        (true, "CLOSED") => IssueState::Closed,
        (true, "OPEN") => IssueState::Open,
        _ => IssueState::Unknown,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LaneClass {
    LandedElsewhere,
    Superseded,
    StaleDirty,
    Owned,
}

impl LaneClass {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::LandedElsewhere => "landed-elsewhere",
            Self::Superseded => "superseded",
            Self::StaleDirty => "stale-dirty",
            Self::Owned => "owned",
        }
    }
}

pub(super) struct LaneFacts<'a> {
    pub(super) dirty: bool,
    pub(super) equivalent: bool,
    pub(super) issues: &'a [IssueState],
    pub(super) idle_hours: f64,
    pub(super) held: bool,
}

pub(super) fn lane_class(facts: &LaneFacts) -> LaneClass {
    if facts.dirty {
        return if facts.idle_hours >= 24.0 && !facts.held { LaneClass::StaleDirty } else { LaneClass::Owned };
    }
    if facts.equivalent {
        LaneClass::LandedElsewhere
    } else if !facts.issues.is_empty() && facts.issues.iter().all(|state| *state == IssueState::Closed) {
        LaneClass::Superseded
    } else {
        LaneClass::Owned
    }
}
