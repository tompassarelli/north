use crate::{cochange, outline, placement, tests_reaching::Reach};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn root() -> Option<PathBuf> {
    let output = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok().filter(|output| output.status.success())?;
    Some(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

pub fn render(root: &Path, paths: &[String], most: usize) -> String {
    let (reach, facts): (Reach, Vec<Facts>) = std::thread::scope(|scope| {
        let reach = scope.spawn(|| Reach::load(root));
        let handles: Vec<_> = paths.iter().map(|path| scope.spawn(|| Facts::read(root, path))).collect();
        let facts = handles.into_iter().map(|handle| handle.join().expect("pack reader")).collect();
        (reach.join().expect("coverage map reader"), facts)
    });
    let each = most / paths.len().max(1);
    facts.iter().map(|facts| facts.block(each, &reach)).collect::<String>().chars().take(most).collect()
}

struct Facts {
    path: String,
    items: Vec<String>,
    lines: Vec<String>,
}

impl Facts {
    fn read(root: &Path, path: &str) -> Self {
        let source = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        let history = cochange::of(root, path);
        let mut lines = Vec::new();
        lines.extend(placement::line(root, path));
        if !history.with.is_empty() {
            let named: Vec<String> = history.with.iter().map(|(file, count)| format!("{file} ({count})")).collect();
            lines.push(format!("changed with it in 90 days: {}", named.join(", ")));
        }
        lines.extend(history.reverts.iter().map(|revert| format!("revert: {revert}")));
        Self { path: path.to_owned(), items: outline::items(&source), lines }
    }

    fn block(&self, budget: usize, reach: &Reach) -> String {
        let mut text = format!("{}\n", self.path);
        for line in self.lines.iter().cloned().chain([reach.line(&self.path)]) {
            text.push_str(&format!("  {line}\n"));
        }
        let room = budget.saturating_sub(text.len() + "  outline: \n".len());
        let mut outline = String::new();
        let mut left = self.items.len();
        for item in &self.items {
            let next = if outline.is_empty() { item.clone() } else { format!(", {item}") };
            if outline.len() + next.len() + " (+999 more)".len() > room {
                break;
            }
            outline.push_str(&next);
            left -= 1;
        }
        if !self.items.is_empty() {
            let more = if left > 0 { format!(" (+{left} more)") } else { String::new() };
            text.push_str(&format!("  outline: {outline}{more}\n"));
        }
        text
    }
}
