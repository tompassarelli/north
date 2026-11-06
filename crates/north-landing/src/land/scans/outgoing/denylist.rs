use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::landing::denylist;
use crate::landing::safe_push_state::cancelled;
use crate::landing::step::{Refused, Step};
use crate::watch::facts::slug_of;

pub struct Denylist {
    pub south: PathBuf,
    pub config: Option<PathBuf>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DenylistVerdict {
    pub lines: Vec<String>,
    pub refused: bool,
}

impl DenylistVerdict {
    fn pass(lines: Vec<String>) -> Self {
        Self { lines, refused: false }
    }

    fn refuse(lines: Vec<String>) -> Self {
        Self { lines, refused: true }
    }
}

impl Denylist {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let south = std::env::var_os("SAFE_PUSH_SOUTH").filter(|value| !value.is_empty()).map_or_else(|| home.join("code/south"), PathBuf::from);
        let config = std::env::var_os("SAFE_PUSH_DENYLIST").filter(|value| !value.is_empty()).map(PathBuf::from);
        Self { south, config }
    }

    pub fn scan(&self, root: &Path, origin_url: &str, base: &str, head: &str, tag: &str) -> Step {
        let verdict = self.verdict(|| self.visibility_of(origin_url), || self.findings(root, base, head, tag));
        if verdict.refused {
            cancelled()?;
        }
        verdict.lines.iter().for_each(|line| println!("{line}"));
        if verdict.refused { Err(Refused(1)) } else { Ok(()) }
    }

    pub fn visibility_of(&self, origin_url: &str) -> String {
        if self.is_south(origin_url) { "PRIVATE".to_owned() } else { visibility(origin_url) }
    }

    fn is_south(&self, origin_url: &str) -> bool {
        let south = Command::new("git").arg("-C").arg(&self.south).args(["remote", "get-url", "origin"]).stderr(Stdio::null()).output();
        let south = south.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
        south.as_deref().and_then(slug_of).zip(slug_of(origin_url)).is_some_and(|(south, pushed)| south.eq_ignore_ascii_case(&pushed))
    }

    pub fn verdict(
        &self,
        visibility: impl FnOnce() -> String,
        findings: impl FnOnce() -> Result<BTreeSet<String>, String>,
    ) -> DenylistVerdict {
        let configured = self.config.as_ref().is_some_and(|config| config.is_file());
        if !configured && !self.south.is_dir() {
            let named = self.config.as_ref().map_or_else(|| format!("{} origin/main", self.south.display()), |config| config.display().to_string());
            return DenylistVerdict::pass(vec![format!(
                "safe-push: ⚠ no deny-list at {named} and no south checkout at {}, so private values were not scanned.",
                self.south.display()
            )]);
        }
        let visibility = visibility();
        if matches!(visibility.as_str(), "PRIVATE" | "INTERNAL") {
            return DenylistVerdict::pass(Vec::new());
        }
        if let Some(config) = self.config.as_ref().filter(|_| !configured) {
            return DenylistVerdict::refuse(vec![format!(
                "safe-push: ✗ the deny-list {} is missing although {} exists, so a {visibility} push cannot be scanned for private values; restore it in south; NOT pushed.",
                config.display(),
                self.south.display()
            )]);
        }
        self.report(findings(), &visibility)
    }

    fn findings(&self, root: &Path, base: &str, head: &str, tag: &str) -> Result<BTreeSet<String>, String> {
        match &self.config {
            Some(config) => denylist::scan_range(root, config, base, head, Some(tag)),
            None => {
                let held = denylist::south_denylist(&self.south)?;
                denylist::scan_range(root, held.path(), base, head, Some(tag))
            }
        }
    }

    fn report(&self, findings: Result<BTreeSet<String>, String>, visibility: &str) -> DenylistVerdict {
        let found = match findings {
            Ok(found) if found.is_empty() => return DenylistVerdict::pass(Vec::new()),
            Ok(found) => found,
            Err(error) => {
                return DenylistVerdict::refuse(vec![
                    format!("denylist: cannot scan: {error}"),
                    format!(
                        "safe-push: ✗ the deny-list scan could not run (exit 2), so a {visibility} push was not checked for private values; NOT pushed."
                    ),
                ]);
            }
        };
        let source = self.config.as_ref().map_or_else(|| format!("in {} origin/main", self.south.display()), |config| config.display().to_string());
        let mut lines: Vec<String> = found.iter().map(|finding| format!("denylist: {finding}")).collect();
        lines.push(format!(
            "safe-push: ✗ PRIVATE VALUE for a {visibility} repository matched the deny-list {source} — it belongs in south; NOT pushed."
        ));
        DenylistVerdict::refuse(lines)
    }
}

fn visibility(origin_url: &str) -> String {
    let output = Command::new("gh")
        .args(["repo", "view", origin_url, "--json", "visibility", "-q", ".visibility"])
        .stderr(Stdio::null())
        .output();
    let observed = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
        .unwrap_or_default();
    if observed.is_empty() { "UNKNOWN".to_owned() } else { observed }
}

#[cfg(test)]
#[path = "denylist_tests.rs"]
mod tests;
