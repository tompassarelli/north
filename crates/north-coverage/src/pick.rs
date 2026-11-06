use std::collections::BTreeSet;

use crate::hunks::{Hunk, Hunks, carry, stretch};
use crate::map::{Function, Map, Test};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Tests { tests: BTreeSet<Test>, file_level: Vec<String> },
    Unmapped(String),
}

pub struct Change<'a> {
    pub map: &'a Map,
    pub since_map: Option<&'a Hunks>,
    pub lane: &'a Hunks,
    pub other_paths: &'a [String],
}

pub fn select(change: &Change) -> Selection {
    if let Some(path) = change.other_paths.first() {
        return Selection::Unmapped(format!("{path} is a build or run-time input the coverage map does not trace"));
    }
    let Some(since_map) = change.since_map else {
        return Selection::Unmapped(format!("the map's commit {} is not in this repository", short(&change.map.commit)));
    };
    let mut tests = BTreeSet::new();
    let mut file_level = Vec::new();
    for (file, hunks) in change.lane {
        if !file.ends_with(".rs") {
            return Selection::Unmapped(format!("{file} is a build input the coverage map does not trace"));
        }
        let functions: Vec<&Function> = change.map.in_file(file).collect();
        if functions.is_empty() {
            return Selection::Unmapped(format!("{file} has no function in the coverage map of {}", short(&change.map.commit)));
        }
        let stale = since_map.get(file).map_or(&[][..], Vec::as_slice);
        let indices = match reached_by_functions(&functions, stale, hunks) {
            Reached::Functions(indices) => indices,
            Reached::ChangedSinceMap => {
                file_level.push(file.clone());
                functions.iter().flat_map(|function| function.reached_by.iter().copied()).collect()
            }
            Reached::Untraced(line) => return Selection::Unmapped(format!("{file}:{line} is outside every function, so the tests it reaches cannot be traced")),
        };
        tests.extend(indices.into_iter().filter_map(|index| change.map.tests.get(index as usize).cloned()));
    }
    Selection::Tests { tests, file_level }
}

enum Reached {
    Functions(BTreeSet<u32>),
    ChangedSinceMap,
    Untraced(u32),
}

fn reached_by_functions(functions: &[&Function], stale: &[Hunk], lane: &[Hunk]) -> Reached {
    let fresh: Vec<(u32, u32, &Function)> = functions.iter().filter_map(|function| carry(function.start, function.end, stale).map(|(start, end)| (start, end, *function))).collect();
    let outdated_spans: Vec<(u32, u32)> = functions.iter().filter(|function| carry(function.start, function.end, stale).is_none()).map(|function| stretch(function.start, function.end, stale)).collect();
    let changed_since_map = |line: u32| stale.iter().any(|hunk| hunk.new_count > 0 && (hunk.new_start..hunk.new_start + hunk.new_count).contains(&line)) || outdated_spans.iter().any(|(start, end)| (*start..=*end).contains(&line));
    let mut reached = BTreeSet::new();
    let mut outdated = false;
    for hunk in lane {
        let probes: Vec<u32> = if hunk.old_count == 0 { vec![hunk.old_start] } else { hunk.lines().collect() };
        for line in probes {
            let inside = |start: u32, end: u32| if hunk.old_count == 0 { start <= line && line < end } else { start <= line && line <= end };
            match fresh.iter().filter(|(start, end, _)| inside(*start, *end)).min_by_key(|(start, end, _)| end - start) {
                Some(innermost) => reached.extend(innermost.2.reached_by.iter().copied()),
                None if changed_since_map(line) || (hunk.old_count == 0 && changed_since_map(line + 1)) => outdated = true,
                None => return Reached::Untraced(line),
            }
        }
    }
    if outdated { Reached::ChangedSinceMap } else { Reached::Functions(reached) }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(9)]
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
