use std::collections::{BTreeMap, BTreeSet};

use crate::hunks::{Hunk, Hunks, carry};
use crate::map::{Function, Map, Test};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub package: String,
    pub target: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Tests { tests: BTreeSet<Test>, file_level: Vec<String>, added: Vec<Added> },
    Unmapped(String),
}

pub struct Change<'a> {
    pub map: &'a Map,
    pub since_map: Option<&'a Hunks>,
    pub lane: &'a Hunks,
    pub other_paths: &'a [String],
    pub mentions: &'a BTreeMap<String, Vec<String>>,
    pub added: &'a [Added],
}

pub fn select(change: &Change) -> Selection {
    let mut whole: BTreeSet<String> = BTreeSet::new();
    for path in change.other_paths {
        if toolchain_input(path) {
            return Selection::Unmapped(format!("{path} is a build or run-time input the coverage map does not trace"));
        }
        match change.mentions.get(path).filter(|files| !files.is_empty()) {
            Some(files) => whole.extend(files.iter().cloned()),
            None => return Selection::Unmapped(format!("{path} is a build or run-time input no Rust source names, so the coverage map cannot trace what reads it")),
        }
    }
    let Some(since_map) = change.since_map else {
        return Selection::Unmapped(format!("the map's commit {} is not in this repository", short(&change.map.commit)));
    };
    let mut indices = BTreeSet::new();
    let mut file_level: Vec<String> = Vec::new();
    for (file, hunks) in change.lane {
        if !file.ends_with(".rs") {
            return Selection::Unmapped(format!("{file} is a build input the coverage map does not trace"));
        }
        let functions: Vec<&Function> = change.map.in_file(file).collect();
        if functions.is_empty() {
            if is_new_file(hunks) {
                continue;
            }
            let prefix = crate_prefix(file);
            let reaching: BTreeSet<u32> = change.map.functions.iter().filter(|function| function.file.starts_with(&prefix)).flat_map(|function| function.reached_by.iter().copied()).collect();
            if reaching.is_empty() {
                return Selection::Unmapped(format!("{file} has no function in the coverage map of {}, and neither has its crate", short(&change.map.commit)));
            }
            file_level.push(file.clone());
            indices.extend(reaching);
            continue;
        }
        let stale = since_map.get(file).map_or(&[][..], Vec::as_slice);
        let spans: Vec<(u32, u32)> = functions.iter().map(|function| (function.start, function.end)).collect();
        match touched_regions(&spans, stale, hunks) {
            Touched::Regions(touched) => indices.extend(touched.into_iter().flat_map(|index| functions[index].reached_by.iter().copied())),
            Touched::Outside => {
                file_level.push(file.clone());
                indices.extend(functions.iter().flat_map(|function| function.reached_by.iter().copied()));
            }
        }
    }
    for file in whole {
        let reaching: Vec<u32> = change.map.in_file(&file).flat_map(|function| function.reached_by.iter().copied()).collect();
        if !reaching.is_empty() {
            file_level.push(file);
            indices.extend(reaching);
        }
    }
    file_level.sort();
    file_level.dedup();
    let tests = indices.into_iter().filter_map(|index| change.map.tests.get(index as usize).cloned()).collect();
    Selection::Tests { tests, file_level, added: change.added.to_vec() }
}

pub fn before_push(selection: &Selection, most: usize) -> BTreeSet<Test> {
    match selection {
        Selection::Tests { tests, added, .. } if tests.len() + added.len() <= most => tests.clone(),
        _ => BTreeSet::new(),
    }
}

pub fn added_before_push(selection: &Selection, most: usize) -> Vec<Added> {
    match selection {
        Selection::Tests { tests, added, .. } if tests.len() + added.len() <= most => added.clone(),
        _ => Vec::new(),
    }
}

fn toolchain_input(path: &str) -> bool {
    const ROOT: [&str; 7] = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml", "rust-toolchain", "flake.nix", "flake.lock"];
    ROOT.contains(&path) || path.ends_with("/Cargo.toml") || path.ends_with("/build.rs") || path.starts_with(".cargo/") || path == "bin/cargo" || path.ends_with(".gitattributes")
}

fn is_new_file(hunks: &[Hunk]) -> bool {
    matches!(hunks, [Hunk { old_start: 0, old_count: 0, .. }])
}

fn crate_prefix(file: &str) -> String {
    let mut parts = file.split('/');
    match (parts.next(), parts.next()) {
        (Some("crates"), Some(name)) => format!("crates/{name}/"),
        (Some(root), _) => format!("{root}/"),
        _ => String::new(),
    }
}

pub enum Touched {
    Regions(BTreeSet<usize>),
    Outside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub line: u32,
    pub insertion: bool,
}

impl Probe {
    fn within(self, start: u32, end: u32) -> bool {
        if self.insertion { start <= self.line && self.line < end } else { start <= self.line && self.line <= end }
    }
}

pub struct Located {
    pub regions: BTreeSet<usize>,
    pub outside: Vec<Probe>,
}

pub fn innermost(regions: &[(u32, u32)], stale: &[Hunk], probe: Probe) -> Option<usize> {
    regions.iter().enumerate().filter_map(|(index, &(start, end))| carry(start, end, stale).map(|(start, end)| (start, end, index))).filter(|(start, end, _)| probe.within(*start, *end)).min_by_key(|(start, end, _)| end - start).map(|(_, _, index)| index)
}

pub fn locate(regions: &[(u32, u32)], stale: &[Hunk], lane: &[Hunk]) -> Located {
    let mut located = Located { regions: BTreeSet::new(), outside: Vec::new() };
    for hunk in lane {
        let insertion = hunk.old_count == 0;
        let lines: Vec<u32> = if insertion { vec![hunk.old_start] } else { hunk.lines().collect() };
        for probe in lines.into_iter().map(|line| Probe { line, insertion }) {
            match innermost(regions, stale, probe) {
                Some(index) => {
                    located.regions.insert(index);
                }
                None => located.outside.push(probe),
            }
        }
    }
    located
}

pub fn touched_regions(regions: &[(u32, u32)], stale: &[Hunk], lane: &[Hunk]) -> Touched {
    let located = locate(regions, stale, lane);
    if located.outside.is_empty() { Touched::Regions(located.regions) } else { Touched::Outside }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(9)]
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
