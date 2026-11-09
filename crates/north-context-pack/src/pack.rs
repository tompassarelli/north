use crate::{cochange, outline, placement, tests_reaching::Reach};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const GRAPH_PATIENCE: Duration = Duration::from_millis(120);
const GRAPH_LINE: usize = 240;
const OUTLINE: &str = "  outline: ";
const CUT: &str = "...";

pub fn root() -> Option<PathBuf> {
    let output = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok().filter(|output| output.status.success())?;
    Some(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

pub fn render(root: &Path, paths: &[String], issues: &[String], most: usize) -> String {
    let until = Instant::now() + GRAPH_PATIENCE;
    let (reach, facts): (Reach, Vec<Facts>) = std::thread::scope(|scope| {
        let reach = scope.spawn(|| Reach::load(root));
        let handles: Vec<_> = paths.iter().map(|path| scope.spawn(|| Facts::read(root, path, issues, until))).collect();
        let facts = handles.into_iter().map(|handle| handle.join().expect("pack reader")).collect();
        (reach.join().expect("coverage map reader"), facts)
    });
    blocks(&facts, most, |path| reach.line(path))
}

fn blocks(facts: &[Facts], most: usize, reach: impl Fn(&str) -> String) -> String {
    let each = most / facts.len().max(1);
    facts.iter().map(|facts| facts.block(each, &reach(&facts.path))).collect::<String>().chars().take(most).collect()
}

struct Facts {
    path: String,
    items: Vec<String>,
    lines: Vec<String>,
    reverts: Vec<String>,
    graph: Vec<String>,
}

impl Facts {
    fn read(root: &Path, path: &str, issues: &[String], until: Instant) -> Self {
        let source = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        let history = cochange::of(root, path);
        let mut lines = Vec::new();
        lines.extend(placement::line(root, path));
        if !history.with.is_empty() {
            let named: Vec<String> = history.with.iter().map(|(file, count)| format!("{file} ({count})")).collect();
            lines.push(format!("changed with it in 90 days: {}", named.join(", ")));
        }
        let graph = north_graph_ground::pack_lines(path, issues, until).into_iter().map(|line| line.chars().take(GRAPH_LINE).collect()).collect();
        Self { path: path.to_owned(), items: outline::items(&source), lines, reverts: history.reverts, graph }
    }

    fn block(&self, budget: usize, reach: &str) -> String {
        let mut text = String::new();
        push(&mut text, &self.path, budget);
        let outline = self.outline(share(remaining(&text, budget), 3));
        if !outline.is_empty() {
            let room = remaining(&text, budget);
            push(&mut text, &format!("{OUTLINE}{outline}"), room);
        }
        let reach_room = remaining(&text, budget).min(reach.chars().count() + 3);
        let each = share(remaining(&text, budget) - reach_room, self.lines.len());
        for line in &self.lines {
            push(&mut text, &format!("  {line}"), each);
        }
        push(&mut text, &format!("  {reach}"), reach_room);
        for revert in &self.reverts {
            let room = remaining(&text, budget);
            if room <= CUT.len() + 4 {
                break;
            }
            push(&mut text, &format!("  revert: {revert}"), room);
        }
        for line in &self.graph {
            let room = remaining(&text, budget);
            if room <= CUT.len() + 4 {
                break;
            }
            push(&mut text, &format!("  {line}"), room);
        }
        text
    }

    fn outline(&self, room: usize) -> String {
        let mut outline = String::new();
        let mut left = self.items.len();
        for item in &self.items {
            let next = if outline.is_empty() { item.clone() } else { format!(", {item}") };
            if outline.chars().count() + next.chars().count() + " (+999 more)".len() > room {
                break;
            }
            outline.push_str(&next);
            left -= 1;
        }
        match left {
            0 => outline,
            _ if outline.is_empty() => format!("+{left} more"),
            _ => format!("{outline} (+{left} more)"),
        }
    }
}

fn remaining(text: &str, budget: usize) -> usize {
    budget - text.chars().count()
}

fn share(room: usize, parts: usize) -> usize {
    room / parts.max(1)
}

fn push(text: &mut String, line: &str, room: usize) {
    if room == 0 {
        return;
    }
    let content = room - 1;
    if line.chars().count() <= content {
        text.push_str(line);
    } else if content > CUT.len() {
        text.extend(line.chars().take(content - CUT.len()));
        text.push_str(CUT);
    } else {
        text.extend(line.chars().take(content));
    }
    text.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(path: &str) -> Facts {
        Facts {
            path: path.to_owned(),
            items: (0..40).map(|index| format!("fn item{index} L{index}")).collect(),
            lines: vec!["module north-long, may use crate-roots, foundation, ledger, ops and 4 more".to_owned(), format!("changed with it in 90 days: {}", "crates/north-wide/src/file.rs (3), ".repeat(6))],
            reverts: (0..13).map(|index| format!("01234567{index} reverted abcdef0123 within 48 h")).collect(),
            graph: (0..4).map(|index| format!("graph line {index} {}", "y".repeat(GRAPH_LINE))).collect(),
        }
    }

    #[test]
    fn a_six_path_pack_keeps_every_path_header_outline_placement_cochange_reverts_and_test_filter_inside_its_share() {
        let facts: Vec<Facts> = (0..6).map(|index| facts(&format!("crates/north-x{index}/src/file.rs"))).collect();
        let each = 3_000 / facts.len();
        for facts in &facts {
            let block = facts.block(each, "cargo nextest run -p north-x");
            assert!(block.chars().count() <= each, "{} took {} of {each}", facts.path, block.chars().count());
            assert!(block.starts_with(&facts.path), "block lost its header: {block}");
            assert!(block.contains("outline:"), "block lost its outline: {block}");
            assert!(block.contains("module north-long"), "block lost its placement: {block}");
            assert!(block.contains("changed with it in 90 days"), "block lost its co-change list: {block}");
            assert!(block.contains("revert: 012345670"), "block lost its reverts: {block}");
            assert!(block.contains("cargo nextest run -p north-x"), "block lost its test filter: {block}");
        }
        let text = blocks(&facts, 3_000, |_| "cargo nextest run -p north-x".to_owned());
        assert!(text.chars().count() <= 3_000 && text.matches("cargo nextest run -p north-x").count() >= facts.len(), "pack dropped a path or its test filter");
    }
}
