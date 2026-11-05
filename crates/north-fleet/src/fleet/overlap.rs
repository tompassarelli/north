//! Whether a box about to start names files that lanes already in flight have changed, and what
//! staffing does about it: say so, and when two or more in-flight lanes already changed one of the
//! files, queue the box behind the earliest of them instead of starting it. Everything here is a pure
//! function of the box's `Files:` line and the lanes' changed files; gathering the lanes is the caller's.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

/// What `north fleet launch` prints, instead of a session name, when it queued the box.
pub const QUEUED: &str = "queued:";

static FILES_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*(?:[-*][ \t]+)?Files:(.*)$").expect("files pattern"));

/// A lane in flight: a running session's worktree or a ticket queued at the lander.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lane {
    pub name: String,
    pub task: Option<String>,
    pub since_ms: u64,
    pub files: BTreeSet<String>,
}

/// One in-flight lane and the files of it that the box also names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Overlap {
    pub lane: String,
    pub task: Option<String>,
    pub since_ms: u64,
    pub files: Vec<String>,
}

/// The staffing setting: whether a box overlapping hot files waits, and how many in-flight lanes make a file hot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub follows_on_overlap: bool,
    pub hot_lanes: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self { follows_on_overlap: true, hot_lanes: 2 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Clear,
    Warn(Vec<Overlap>),
    Queue { behind: Overlap, hot: Vec<String>, overlaps: Vec<Overlap> },
}

impl Verdict {
    pub fn overlaps(&self) -> &[Overlap] {
        match self {
            Self::Clear => &[],
            Self::Warn(overlaps) | Self::Queue { overlaps, .. } => overlaps,
        }
    }
}

/// The worktree under `worktrees` that a session's recent output names most often, the lane it is working in.
pub fn worktree_in_use(output: &str, worktrees: &str) -> Option<String> {
    let pattern = Regex::new(&format!("{}/([A-Za-z0-9._-]+)", regex::escape(worktrees.trim_end_matches('/')))).ok()?;
    let mut seen: Vec<(String, usize, usize)> = Vec::new();
    for (order, found) in pattern.captures_iter(output).enumerate() {
        match seen.iter_mut().find(|(slug, ..)| *slug == found[1]) {
            Some(entry) => (entry.1, entry.2) = (entry.1 + 1, order),
            None => seen.push((found[1].to_owned(), 1, order)),
        }
    }
    seen.into_iter().max_by_key(|(_, count, last)| (*count, *last)).map(|(slug, ..)| slug)
}

/// The paths a repository's `.gitattributes` gives a merge driver, which regenerates them on a
/// merge, so two lanes changing one never conflict on it.
#[derive(Clone, Debug, Default)]
pub struct MergeDriven {
    patterns: Vec<Regex>,
}

impl MergeDriven {
    pub fn parse(attributes: &str) -> Self {
        let patterns = attributes
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                let pattern = words.next().filter(|word| !word.starts_with('#'))?;
                words.any(|word| word.strip_prefix("merge=").is_some_and(|driver| !matches!(driver, "text" | "binary"))).then(|| glob(pattern))?
            })
            .collect();
        Self { patterns }
    }

    pub fn covers(&self, path: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.is_match(path))
    }
}

fn glob(pattern: &str) -> Option<Regex> {
    let anchored = pattern.starts_with('/') || pattern.trim_end_matches('/').contains('/');
    let body = pattern.trim_start_matches('/').trim_end_matches('/');
    let mut regex = String::from(if anchored { "^" } else { "^(?:.*/)?" });
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                regex.push_str(".*");
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push_str("(?:/.*)?$");
    Regex::new(&regex).ok()
}

/// The paths a box's `Files:` line names; a directory ends in `/`, and notes beside the paths are dropped.
pub fn declared_files(brief: &str) -> Vec<String> {
    let Some(line) = FILES_LINE.captures(brief) else { return Vec::new() };
    let mut files: Vec<String> = line[1]
        .split([',', ' ', '\t'])
        .map(|word| word.trim_matches(|c: char| matches!(c, '`' | ',' | ';' | '(' | ')' | '*')))
        .filter(|word| (word.contains('/') || word.contains('.')) && !word.ends_with('.') && !word.starts_with("e.g"))
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    files
}

fn names(declared: &str, file: &str) -> bool {
    file == declared || (declared.ends_with('/') && file.starts_with(declared)) || file.strip_prefix(declared).is_some_and(|rest| rest.starts_with('/'))
}

/// The in-flight lanes that changed a file the box names, each with those files; `excused` marks a
/// file that two lanes changing it never conflict on, because a merge driver regenerates it.
pub fn overlaps(declared: &[String], lanes: &[Lane], excused: &dyn Fn(&str) -> bool) -> Vec<Overlap> {
    let mut found: Vec<Overlap> = lanes
        .iter()
        .filter_map(|lane| {
            let files: Vec<String> = lane.files.iter().filter(|file| !excused(file) && declared.iter().any(|named| names(named, file))).cloned().collect();
            (!files.is_empty()).then(|| Overlap { lane: lane.name.clone(), task: lane.task.clone(), since_ms: lane.since_ms, files })
        })
        .collect();
    found.sort_by(|a, b| (a.since_ms, &a.lane).cmp(&(b.since_ms, &b.lane)));
    found
}

/// What to do with a box that names `declared` files while `lanes` are in flight.
pub fn assess(declared: &[String], lanes: &[Lane], excused: &dyn Fn(&str) -> bool, policy: Policy) -> Verdict {
    let found = overlaps(declared, lanes, excused);
    if found.is_empty() {
        return Verdict::Clear;
    }
    let changers = |file: &str| found.iter().filter(|overlap| overlap.files.iter().any(|changed| changed == file)).count();
    let mut hot: Vec<String> = found.iter().flat_map(|overlap| overlap.files.iter()).filter(|file| changers(file) >= policy.hot_lanes).cloned().collect();
    hot.sort();
    hot.dedup();
    let behind = found.iter().find(|overlap| overlap.task.is_some() && overlap.files.iter().any(|file| hot.contains(file)));
    match behind {
        Some(behind) if policy.follows_on_overlap => Verdict::Queue { behind: behind.clone(), hot, overlaps: found.clone() },
        _ => Verdict::Warn(found),
    }
}

/// The warning lines for `verdict`, one per overlapping lane and file.
pub fn warnings(verdict: &Verdict) -> Vec<String> {
    verdict.overlaps().iter().flat_map(|overlap| overlap.files.iter().map(move |file| format!("overlap: lane {} also changes {file}", overlap.lane))).collect()
}

/// The `Follows:` line that queues a box behind `behind`, which must carry a task.
pub fn follows_line(behind: &Overlap, hot: &[String]) -> Option<String> {
    let task = behind.task.as_deref()?;
    Some(format!("Follows: {task} (lane {} changes {})", behind.lane, hot.join(", ")))
}

