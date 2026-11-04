use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::Path;

struct Entry {
    parent: i32,
    group: i32,
    state: char,
    command: String,
}

fn read(directory: &Path) -> Option<Entry> {
    let stat = fs::read_to_string(directory.join("stat")).ok()?;
    let (head, tail) = stat.rsplit_once(") ")?;
    let comm = head.split_once(" (")?.1;
    let mut fields = tail.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let parent = fields.next()?.parse().ok()?;
    let group = fields.next()?.parse().ok()?;
    let argv = fs::read(directory.join("cmdline")).unwrap_or_default();
    let words: Vec<String> = argv.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
    let command = if words.is_empty() { format!("[{comm}]") } else { words.join(" ") };
    Some(Entry { parent, group, state, command })
}

fn table(proc: &Path) -> BTreeMap<i32, Entry> {
    let Ok(entries) = fs::read_dir(proc) else { return BTreeMap::new() };
    entries.flatten().filter_map(|entry| Some((entry.file_name().to_str()?.parse().ok()?, read(&entry.path())?))).collect()
}

fn descends(table: &BTreeMap<i32, Entry>, mut pid: i32, root: i32) -> bool {
    for _ in 0..table.len() {
        if pid == root {
            return true;
        }
        match table.get(&pid) {
            Some(entry) if entry.parent != pid => pid = entry.parent,
            _ => return false,
        }
    }
    false
}

pub fn render(proc: &Path, leader: i32) -> String {
    let table = table(proc);
    let members: BTreeSet<i32> = table.iter().filter(|(pid, entry)| entry.group == leader || descends(&table, **pid, leader)).map(|(pid, _)| *pid).collect();
    let mut children: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    let mut tops = Vec::new();
    for pid in &members {
        let parent = table[pid].parent;
        if members.contains(&parent) && parent != *pid {
            children.entry(parent).or_default().push(*pid);
        } else {
            tops.push(*pid);
        }
    }
    let mut lines = String::new();
    let mut stack: Vec<(i32, usize)> = tops.into_iter().rev().map(|pid| (pid, 0)).collect();
    while let Some((pid, depth)) = stack.pop() {
        let entry = &table[&pid];
        let _ = writeln!(lines, "{}{pid} {} {}", "  ".repeat(depth), entry.state, entry.command);
        if let Some(below) = children.get(&pid) {
            stack.extend(below.iter().rev().map(|child| (*child, depth + 1)));
        }
    }
    lines
}

#[cfg(test)]
#[path = "process_tree_tests.rs"]
mod tests;
