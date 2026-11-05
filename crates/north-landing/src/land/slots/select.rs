use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::infer::infer;
use north_base::build_inputs::rust_build_input;

const SEPARATOR: &str = " :: ";
const RUST_INPUT: &str = "@rust-input";
const REF_SHORT: usize = 12;
pub const OFFLOAD_SLOTS_BUSY: i32 = 96;

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub workflow: String,
    pub publish: Option<String>,
    pub runner: Option<String>,
    pub capacity: Option<String>,
    checks: Vec<String>,
    after: Vec<String>,
    recheck: Vec<String>,
    local_only: Vec<String>,
    machine_bound: Vec<String>,
    inferred: Vec<String>,
}

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new("git").arg("-C").arg(repo).args(arguments).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

fn blob_values(repo: &Path, head: &str, key: &str) -> Vec<String> {
    let blob = format!("{head}:.safe-push");
    git_lines(repo, &["config", "--blob", &blob, "--get-all", key])
}

impl Config {
    pub fn load(repo: &Path, head: &str) -> Self {
        let first = |key: &str| blob_values(repo, head, key).into_iter().next_back();
        let checks = blob_values(repo, head, "land.check");
        let inference_off = first("land.inferred").is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "false" | "no" | "off" | "0"));
        let inferred = if checks.is_empty() && !inference_off { inferred_checks(repo, head) } else { Vec::new() };
        Self {
            workflow: first("land.workflow").unwrap_or_default(),
            publish: first("land.publish").filter(|prefix| !prefix.is_empty()),
            runner: first("land.runner"),
            capacity: first("landing.capacity").filter(|command| !command.is_empty()),
            checks,
            after: blob_values(repo, head, "land.after"),
            recheck: blob_values(repo, head, "land.recheck"),
            local_only: blob_values(repo, head, "land.local-only"),
            machine_bound: blob_values(repo, head, "land.machine-bound"),
            inferred,
        }
    }

    pub fn added_since(&self, earlier: &Self) -> Self {
        Self { checks: self.checks.iter().filter(|entry| !earlier.checks.contains(entry)).cloned().collect(), inferred: Vec::new(), ..self.clone() }
    }

    pub fn after_set(&self) -> Self {
        Self { checks: self.after.clone(), after: Vec::new(), inferred: Vec::new(), ..self.clone() }
    }

    pub fn recheck_set(&self) -> Self {
        if self.recheck.is_empty() {
            return self.clone();
        }
        Self { checks: self.recheck.clone(), recheck: Vec::new(), inferred: Vec::new(), ..self.clone() }
    }

    pub fn has_checks(&self) -> bool {
        !self.checks.is_empty() || !self.inferred.is_empty()
    }

    pub fn is_inferred(&self, command: &str) -> bool {
        self.inferred.iter().any(|inferred| inferred == command)
    }

    pub fn runs(&self, command: &str) -> bool {
        self.checks.iter().any(|entry| entry.split_once(SEPARATOR).is_some_and(|(_, listed)| listed == command)) || self.is_inferred(command)
    }

    pub fn local_reason(&self, command: &str) -> Option<&str> {
        reason(&self.local_only, command)
    }

    pub fn off_actions_reason(&self, command: &str) -> Option<&str> {
        self.local_reason(command).or_else(|| reason(&self.machine_bound, command))
    }

    pub fn publish_ref(&self, head: &str) -> Option<String> {
        self.publish.as_ref().map(|prefix| format!("{prefix}{}", &head[..head.len().min(REF_SHORT)]))
    }

    pub fn commands(&self, repo: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
        if self.checks.is_empty() {
            return Ok(self.inferred.clone());
        }
        let mut commands: Vec<String> = Vec::new();
        for entry in &self.checks {
            let Some((patterns, command)) = entry.split_once(SEPARATOR) else {
                return Err(format!("invalid land.check: {entry}"));
            };
            if patterns.is_empty() || command.is_empty() {
                return Err(format!("invalid land.check: {entry}"));
            }
            if commands.iter().any(|seen| seen == command) || !touches(repo, base, head, patterns) {
                continue;
            }
            commands.push(command.to_owned());
        }
        Ok(commands)
    }

    pub fn coverage(&self, repo: &Path, tree: &str) -> BTreeMap<String, BTreeSet<String>> {
        let empty = empty_tree(repo);
        let mut coverage: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if self.checks.is_empty() {
            let files: BTreeSet<String> = changed(repo, &empty, tree, &[]).into_iter().collect();
            for command in &self.inferred {
                coverage.entry(command.clone()).or_default().extend(files.iter().cloned());
            }
            return coverage;
        }
        for (patterns, command) in self.checks.iter().filter_map(|entry| entry.split_once(SEPARATOR)) {
            let (rust, specs): (Vec<&str>, Vec<&str>) = patterns.split(',').filter(|spec| !spec.is_empty()).partition(|spec| *spec == RUST_INPUT);
            let covered = coverage.entry(command.to_owned()).or_default();
            if !rust.is_empty() {
                covered.extend(changed(repo, &empty, tree, &[]).into_iter().filter(|path| rust_build_input(path)));
            }
            if !specs.is_empty() {
                covered.extend(changed(repo, &empty, tree, &specs));
            }
        }
        coverage
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub workflow: bool,
    pub runner: bool,
    pub reserve: bool,
    pub free_slots: Option<u32>,
}

impl Placement {
    pub fn probes_slots(&self) -> bool {
        self.workflow && self.runner && !self.reserve
    }

    pub fn on_actions(&self, portable: bool) -> bool {
        self.workflow && portable && !self.reserve && (!self.runner || self.free_slots == Some(0))
    }

    pub fn overflows(&self, status: i32, offloaded: bool, portable: bool) -> bool {
        status == OFFLOAD_SLOTS_BUSY && offloaded && self.workflow && portable && !self.reserve
    }
}

fn inferred_checks(repo: &Path, head: &str) -> Vec<String> {
    let files = git_lines(repo, &["ls-tree", "--name-only", head]);
    let flake = files.iter().any(|file| file == "flake.nix").then(|| git_lines(repo, &["show", &format!("{head}:flake.nix")]).join("\n"));
    let package = files.iter().any(|file| file == "package.json").then(|| git_lines(repo, &["show", &format!("{head}:package.json")]).join("\n"));
    infer(&files, flake.as_deref(), package.as_deref())
}

fn reason<'a>(entries: &'a [String], command: &str) -> Option<&'a str> {
    let suffix = format!("{SEPARATOR}{command}");
    entries.iter().find(|entry| entry.ends_with(&suffix)).map(|entry| entry.split(SEPARATOR).next().unwrap_or_default())
}

pub fn path_class(repo: &Path, base: &str, head: &str) -> String {
    let mut tops: Vec<String> = changed(repo, base, head, &[]).iter().filter_map(|path| path.split('/').next()).map(str::to_owned).collect();
    tops.sort_unstable();
    tops.dedup();
    tops.join("+")
}

fn touches(repo: &Path, base: &str, head: &str, patterns: &str) -> bool {
    let (rust, specs): (Vec<&str>, Vec<&str>) = patterns.split(',').partition(|spec| *spec == RUST_INPUT);
    (!rust.is_empty() && changed(repo, base, head, &[]).iter().any(|path| rust_build_input(path))) || (!specs.is_empty() && !changed(repo, base, head, &specs).is_empty())
}

fn empty_tree(repo: &Path) -> String {
    git_lines(repo, &["hash-object", "-t", "tree", "--stdin"]).into_iter().next().unwrap_or_default()
}

pub(super) fn changed(repo: &Path, base: &str, head: &str, specs: &[&str]) -> Vec<String> {
    let mut arguments: Vec<&str> = if base.is_empty() {
        vec!["ls-tree", "-r", "--name-only", head, "--"]
    } else {
        vec!["diff", "--name-only", "--no-renames", base, head, "--"]
    };
    arguments.extend(specs);
    git_lines(repo, &arguments).into_iter().filter(|line| !line.is_empty()).collect()
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
