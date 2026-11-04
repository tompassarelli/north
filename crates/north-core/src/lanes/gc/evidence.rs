use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;

use super::exec::{command, git};

const CHERRY_LIMIT: u64 = 400;
static ISSUES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:Refs|Fixes|Closes|Part of)\s+([\w.-]*#\d+)").unwrap());
static REFS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bRefs\s+([\w.-]+#\d+)").unwrap());
static ORIGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)tompassarelli/([^/]+?)(?:\.git)?$").unwrap()
});

#[derive(Clone)]
pub(super) struct Landed {
    pub(super) landed: bool,
    pub(super) ahead: u64,
    pub(super) how: String,
}

fn counts(repo: &Path, base: &str, tip: &str) -> (u64, u64) {
    let counted = git(repo, &["rev-list", "--left-right", "--count", &format!("{base}...{tip}")]).out;
    let mut sides = counted.split_whitespace().map(|side| side.parse().unwrap_or(0));
    (sides.next().unwrap_or(0), sides.next().unwrap_or(0))
}

fn base_tree(repo: &Path, base: &str) -> String {
    git(repo, &["rev-parse", &format!("{base}^{{tree}}")]).out
}

pub(super) fn landed(repo: &Path, base: &str, tip: &str) -> Landed {
    landed_against(repo, base, None, tip)
}

fn landed_against(repo: &Path, base: &str, tree_of_base: Option<&str>, tip: &str) -> Landed {
    let yes = |ahead, how: &str| Landed { landed: true, ahead, how: how.to_string() };
    if git(repo, &["merge-base", "--is-ancestor", tip, base]).ok {
        return yes(0, "ancestor");
    }
    let (behind, ahead) = counts(repo, base, tip);
    let tree = command(&["north", "landed-tree", "--repo", &repo.to_string_lossy(), base, tip]);
    if tree.ok {
        return yes(ahead, &tree.out);
    }
    let merged = git(repo, &["merge-tree", "--write-tree", base, tip]);
    if merged.ok && merged.out.lines().next() == Some(tree_of_base.map_or_else(|| base_tree(repo, base), str::to_owned).as_str()) {
        return yes(ahead, "adds nothing");
    }
    if behind <= CHERRY_LIMIT {
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

pub(super) fn own_repo(main: &Path, name: &str) -> String {
    origin_repo(main).unwrap_or_else(|| name.to_string())
}

pub(super) fn named_issues(main: &Path, own: &str, base: &str, tip: &str) -> Result<Vec<String>, String> {
    let text = bodies(main, base, tip, "issue evidence unavailable")?;
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

pub(super) const SUPERSEDED_BRANCH_HOURS: f64 = 14.0 * 24.0;

impl LaneClass {
    pub(super) fn report_label(self) -> &'static str {
        match self {
            Self::LandedElsewhere => "landed-by-tree",
            Self::Superseded => "superseded",
            Self::StaleDirty | Self::Owned => "lost",
        }
    }
}

pub(super) fn branch_hold(class: LaneClass, checked_out: bool, idle_hours: f64) -> &'static str {
    if checked_out {
        "has a worktree"
    } else if matches!(class, LaneClass::Owned | LaneClass::StaleDirty) {
        "lost work"
    } else if class == LaneClass::Superseded && idle_hours < SUPERSEDED_BRANCH_HOURS {
        "superseded under 14 days"
    } else {
        ""
    }
}

pub(super) fn landed_each(repo: &Path, base: &str, tips: &[String]) -> HashMap<String, Landed> {
    let tree_of_base = base_tree(repo, base);
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(1, NonZeroUsize::get).min(tips.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut verdicts = Vec::new();
                    while let Some(tip) = tips.get(next.fetch_add(1, Ordering::Relaxed)) {
                        verdicts.push((tip.clone(), landed_against(repo, base, Some(&tree_of_base), tip)));
                    }
                    verdicts
                })
            })
            .collect();
        handles.into_iter().flat_map(|handle| handle.join().unwrap_or_default()).collect()
    })
}

pub(super) fn committed_each(repo: &Path, tips: &[String]) -> HashMap<String, i64> {
    let mut arguments = vec!["log", "--no-walk=unsorted", "--format=%H %ct"];
    arguments.extend(tips.iter().map(String::as_str));
    let listed = git(repo, &arguments);
    if !listed.ok {
        return HashMap::new();
    }
    listed.out.lines().filter_map(|line| line.split_once(' ')).map(|(tip, when)| (tip.to_owned(), when.parse().unwrap_or(0))).collect()
}

pub(super) fn committed(repo: &Path, tip: &str) -> i64 {
    git(repo, &["log", "-1", "--format=%ct", tip]).out.parse().unwrap_or(0)
}
