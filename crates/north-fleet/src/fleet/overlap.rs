//! Which files a box about to start names, and which of them lanes already in flight changed. Everything here is a pure
//! function of the box's `Files:` line and the lanes' changed files; gathering the lanes is the caller's.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

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
