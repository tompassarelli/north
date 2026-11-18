use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::landing::red_check::{self, Failure};
use super::candidate::git;

const SCOPE: &str = "safe-push: the Rust checks cover ";
const CHECK: &str = "safe-push: land check";
const WORKSPACE: &str = "--workspace";

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Episode {
    pub head: String,
    pub scope: BTreeSet<String>,
    pub failing: BTreeSet<Failure>,
    pub reds: BTreeSet<String>,
}

impl Episode {
    pub fn opened(head: &str, log: &str, failing: BTreeSet<Failure>) -> Self {
        Self { head: head.to_owned(), scope: scope(log), failing, reds: BTreeSet::new() }
    }

    pub fn read(marker: &Path) -> Option<Self> {
        let text = fs::read_to_string(marker).ok()?;
        let mut episode = Self::default();
        for line in text.lines() {
            match line.split_once('\t') {
                Some(("head", head)) => head.clone_into(&mut episode.head),
                Some(("scope", package)) => {
                    episode.scope.insert(package.to_owned());
                }
                Some(("failure", key)) => episode.failing.extend(Failure::from_key(key)),
                Some(("red", red)) => {
                    episode.reds.insert(red.to_owned());
                }
                _ => {}
            }
        }
        Some(episode)
    }

    pub fn write(&self, marker: &Path) -> std::io::Result<()> {
        let mut text = format!("head\t{}\n", self.head);
        self.scope.iter().for_each(|package| text.push_str(&format!("scope\t{package}\n")));
        self.failing.iter().for_each(|failure| text.push_str(&format!("failure\t{}\n", failure.key())));
        self.reds.iter().for_each(|red| text.push_str(&format!("red\t{red}\n")));
        fs::write(marker, text)
    }

    pub fn write_cleared(&self, marker: &Path, by: &str) -> std::io::Result<()> {
        self.write(marker)?;
        fs::OpenOptions::new().append(true).open(marker).and_then(|mut file| std::io::Write::write_all(&mut file, format!("cleared\t{by}\n").as_bytes()))
    }

    /// Whether the episode `cleared` records ended at a head that descends from `red`, over every check in `failing`: a red verdict on `red` arriving after that is main's past, not its state.
    pub fn covers_red(cleared: &Path, repo: &Path, red: &str, failing: &BTreeSet<Failure>) -> bool {
        let Some(episode) = Self::read(cleared) else { return false };
        let by = fs::read_to_string(cleared).unwrap_or_default().lines().find_map(|line| line.strip_prefix("cleared\t").map(str::to_owned));
        by.is_some_and(|by| by != red && failing.is_subset(&episode.failing) && git(repo, &["merge-base", "--is-ancestor", red, &by]).is_ok())
    }

    pub fn ended_by(&self, green_log: &str, every: bool, repo: &Path, head: &str) -> bool {
        let descends = |red: &String| red != head && git(repo, &["merge-base", "--is-ancestor", red, head]).is_ok();
        let after_every_red = std::iter::once(&self.head).chain(&self.reds).all(descends);
        let whole = every && scope(green_log) == BTreeSet::from([WORKSPACE.to_owned()]) && after_every_red;
        let removed = if after_every_red { self.removed_steps(repo, head) } else { BTreeSet::new() };
        let all_removed = !self.failing.is_empty() && self.failing.iter().all(|failure| matches!(failure, Failure::Step(command) if removed.contains(command)));
        all_removed || self.covered_by(green_log, whole, &removed) || reverted_since(repo, &self.head, head)
    }

    /// The failing steps whose command a `land.check` of `.safe-push` listed at a red head of the episode and no longer lists at `head`, so no green run can rerun them: on 2026-11-18 the old-state check left .safe-push after its tests were deleted, and main stayed red on it, first alone and then beside a later red step (north#1650).
    fn removed_steps(&self, repo: &Path, head: &str) -> BTreeSet<String> {
        let Some(now) = land_checks(repo, head) else { return BTreeSet::new() };
        let listed: BTreeSet<String> = std::iter::once(&self.head).chain(&self.reds).filter_map(|red| land_checks(repo, red)).flatten().collect();
        self.failing.iter().filter_map(|failure| match failure { Failure::Step(command) if listed.contains(command) && !now.contains(command) => Some(command.clone()), _ => None }).collect()
    }

    pub fn rerun_command(&self, host: &str) -> Option<String> {
        red_check::rerun_on(host, &self.named().into_iter().cloned().collect())
    }

    pub fn named(&self) -> Vec<&Failure> {
        self.failing.iter().filter(|failure| matches!(failure, Failure::Cargo(_) | Failure::Harness(_))).collect()
    }

    fn covered_by(&self, green_log: &str, whole: bool, removed: &BTreeSet<String>) -> bool {
        let named = self.named();
        if !named.is_empty() {
            let passed = passed(green_log);
            return named.iter().all(|failure| passed.contains(*failure) || (whole && matches!(failure, Failure::Cargo(_))));
        }
        let ran = |command: &str| green_log.lines().map(red_check::log_text).any(|line| line.starts_with(CHECK) && line.contains(&format!(": {command} (")));
        let steps_ran = self.failing.iter().all(|failure| !matches!(failure, Failure::Step(command) if !ran(command) && !removed.contains(command)));
        steps_ran && self.scope.is_subset(&scope(green_log))
    }
}

fn scope(log: &str) -> BTreeSet<String> {
    let covered = log.lines().map(red_check::log_text).find_map(|line| line.strip_prefix(SCOPE)).unwrap_or_default();
    covered.split_whitespace().filter(|word| *word != "-p").map(str::to_owned).collect()
}

fn passed(log: &str) -> BTreeSet<Failure> {
    let mut found = BTreeSet::new();
    for line in log.lines().map(red_check::log_text) {
        if let Some(rest) = line.strip_prefix("PASS ") {
            found.extend(rest.split_whitespace().last().map(|name| Failure::Cargo(name.to_owned())));
        } else if let Some(rest) = line.strip_prefix("test ") {
            let name = rest.split_once(" ... ok").map(|(name, _)| name.trim());
            found.extend(name.map(|name| Failure::Cargo(name.to_owned())));
        } else if let Some((path, _)) = line.strip_prefix("land check: ").and_then(|rest| rest.split_once(" passed")).filter(|(_, after)| after.is_empty() || after.starts_with(" after ")) {
            found.insert(Failure::Harness(path.to_owned()));
        }
    }
    found
}

/// The commands of the `land.check` values of `.safe-push` at `at`, as git's config parser reads them, after any `PATHS :: `; none when the file is missing or git cannot read it.
fn land_checks(repo: &Path, at: &str) -> Option<BTreeSet<String>> {
    let blob = format!("{at}:.safe-push");
    git(repo, &["cat-file", "-e", &blob]).ok()?;
    let output = std::process::Command::new("git").arg("-C").arg(repo).args(["config", "--blob", &blob, "-z", "--get-all", "land.check"]).output().ok()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }
    let values = String::from_utf8_lossy(&output.stdout);
    Some(values.split('\0').filter(|value| !value.is_empty()).map(|value| value.split_once(" :: ").map_or(value, |(_, command)| command).trim().to_owned()).collect())
}

fn reverted_since(repo: &Path, red: &str, head: &str) -> bool {
    let Ok(messages) = git(repo, &["log", "--format=%B", &format!("{red}..{head}")]) else { return false };
    messages.lines().filter_map(|line| line.strip_prefix("Sheriff-Reverts:")).map(str::trim).any(|reverted| git(repo, &["merge-base", "--is-ancestor", reverted, red]).is_ok())
}
