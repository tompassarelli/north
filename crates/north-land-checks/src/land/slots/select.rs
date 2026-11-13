use std::path::Path;
use std::process::Command;

use super::infer::infer;
use north_base::foundation::exit::SLOTS_BUSY_STATUS;
use north_base::build_inputs::{ENQUEUE_PATTERN, rust_build_input};
use north_base::store_pins::{pinned_entries, store_input};

const SEPARATOR: &str = " :: ";
const RUST_INPUT: &str = "@rust-input";
const REF_SHORT: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub workflow: String,
    pub publish: Option<String>,
    pub runner: Option<String>,
    pub capacity: Option<String>,
    checks: Vec<String>,
    after: Vec<String>,
    after_landing: Vec<String>,
    after_only: Vec<String>,
    local_only: Vec<String>,
    machine_bound: Vec<String>,
    host_inputs: Vec<String>,
    deadlines: Vec<String>,
    inferred: Vec<String>,
    local_checks: bool,
}

pub(super) fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    git_output(repo, arguments).unwrap_or_default()
}

fn git_output(repo: &Path, arguments: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
}

struct Blob(Vec<(String, String)>);

/// One `.safe-push` entry with its command as the build system this host runs issues it, so every list that names a check line names the same one.
fn issued(side: &dyn north_build_system::BuildSystem, entry: &str) -> String {
    match entry.split_once(SEPARATOR) {
        Some((lead, command)) => format!("{lead}{SEPARATOR}{}", side.land_line(command)),
        None => entry.to_owned(),
    }
}

impl Blob {
    /// The `.safe-push` of `head`, or None when this repository does not hold that commit: a head it has not fetched
    /// is unknown, never a head that carries no entries, so a caller comparing two heads never takes an unread head
    /// for a landing that dropped every line (north#1247). A commit it holds with no `.safe-push` reads as no entries.
    fn try_read(repo: &Path, head: &str) -> Option<Self> {
        let known = Command::new("git").arg("-C").arg(repo).args(["cat-file", "-e", &format!("{head}^{{commit}}")]).output().is_ok_and(|output| output.status.success());
        if !known {
            return None;
        }
        let blob = format!("{head}:.safe-push");
        let Some(output) = Command::new("git").arg("-C").arg(repo).args(["config", "--blob", &blob, "--list", "-z"]).output().ok().filter(|output| output.status.success()) else {
            return Some(Self(Vec::new()));
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let side = north_build_system::for_tree(repo);
        let side = side.as_ref();
        Some(Self(text.split('\0').filter_map(|entry| entry.split_once('\n')).flat_map(|(key, value)| value.lines().map(move |line| (key.to_owned(), issued(side, line)))).collect()))
    }

    fn read(repo: &Path, head: &str) -> Self {
        Self::try_read(repo, head).unwrap_or_else(|| Self(Vec::new()))
    }

    fn values(&self, key: &str) -> Vec<String> {
        self.0.iter().filter(|(name, _)| name == key).map(|(_, value)| value.clone()).collect()
    }
}

/// Expands one `STORE@PINS` host input into the store root and the entries the check's head pins:
/// `PINS` names a directory at `head` with one file, or one directory of shard files, per input family, and each file's content is the
/// 64-hex name of the store entry that family's build reads, so a host whose store exists but lacks
/// one of those entries takes no check that reads it (north#892). A declaration that resolves to no
/// entry stays whole, so the probe reports it missing instead of silently dropping the store.
fn expand_store_input(repo: &Path, head: &str, input: &str) -> Vec<String> {
    let Some((store, pins)) = store_input(input) else { return vec![input.to_owned()] };
    let mut expanded = vec![store.to_owned()];
    expanded.extend(pinned_entries(repo, head, pins).into_iter().map(|entry| format!("{store}/{entry}")));
    if expanded.len() == 1 { vec![input.to_owned()] } else { expanded }
}

fn expand_host_inputs(repo: &Path, head: &str, line: String) -> String {
    let Some((inputs, command)) = line.split_once(SEPARATOR) else { return line };
    let expanded: Vec<String> = inputs.split_whitespace().flat_map(|input| expand_store_input(repo, head, input)).collect();
    format!("{}{SEPARATOR}{command}", expanded.join(" "))
}

impl Config {
    pub fn load(repo: &Path, head: &str) -> Self {
        let blob = Blob::read(repo, head);
        let first = |key: &str| blob.values(key).into_iter().next_back();
        let checks = blob.values("land.check");
        let inference_off = first("land.inferred").is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "false" | "no" | "off" | "0"));
        let inferred = if checks.is_empty() && !inference_off { inferred_checks(repo, head) } else { Vec::new() };
        Self {
            workflow: first("land.workflow").unwrap_or_default(),
            publish: first("land.publish").filter(|prefix| !prefix.is_empty()),
            runner: first("land.runner"),
            capacity: first("landing.capacity").filter(|command| !command.is_empty()),
            checks,
            after: blob.values("land.after"),
            after_landing: blob.values("after-landing.check"),
            after_only: blob.values("land.after-only"),
            local_only: blob.values("land.local-only"),
            machine_bound: blob.values("land.machine-bound"),
            host_inputs: blob.values("land.host-input").into_iter().map(|line| expand_host_inputs(repo, head, line)).collect(),
            deadlines: blob.values("land.deadline"),
            inferred,
            local_checks: first("land.check-local").is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1")),
        }
    }

    pub fn added_since(&self, earlier: &Self) -> Self {
        Self { checks: self.checks.iter().filter(|entry| !earlier.checks.contains(entry)).cloned().collect(), inferred: Vec::new(), ..self.clone() }
    }

    /// The `land.after` lines of `head`; None when this repository cannot read that head's `.safe-push`, so the
    /// lander never cancels an after-landing run for a landing whose lines it could not compare (north#1247).
    pub fn after_lines(repo: &Path, head: &str) -> Option<Vec<String>> {
        Blob::try_read(repo, head).map(|blob| blob.values("land.after"))
    }

    pub fn after_set(&self) -> Self {
        Self { checks: self.checks.iter().chain(&self.after).cloned().collect(), after: Vec::new(), inferred: Vec::new(), ..self.clone() }
    }

    /// The lines run before a lane queues: land.check lines marked `@enqueue`, and land.after lines so marked when `with_after` is set.
    pub fn queue_set(&self, with_after: bool) -> Self {
        let queued = queued(&self.checks).chain(queued(if with_after { &self.after } else { &[] })).cloned().collect();
        Self { checks: queued, after: Vec::new(), inferred: Vec::new(), ..self.clone() }
    }

    pub fn local_checks(&self) -> bool {
        self.local_checks
    }

    pub fn without_placement(&self) -> Self {
        Self { workflow: String::new(), publish: None, runner: None, capacity: None, ..self.clone() }
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

    pub fn after_without_reason(&self) -> Vec<&str> {
        let commands = self.after.iter().chain(&self.after_landing).filter_map(|entry| entry.split_once(SEPARATOR).map(|(_, command)| command));
        commands.filter(|command| !self.runs(command) && reason(&self.after_only, command).is_none()).collect()
    }

    pub fn after_only_reason(&self, command: &str) -> Option<&str> {
        reason(&self.after_only, command)
    }

    pub fn local_reason(&self, command: &str) -> Option<&str> {
        reason(&self.local_only, command)
    }

    pub fn deadline_seconds(&self, command: &str) -> Option<u64> {
        reason(&self.deadlines, command).and_then(|seconds| seconds.trim().parse().ok())
    }

    pub fn host_inputs(&self, command: &str) -> Vec<String> {
        reason(&self.host_inputs, command).map(|inputs| inputs.split_whitespace().map(str::to_owned).collect()).unwrap_or_default()
    }

    pub fn off_actions_reason(&self, command: &str) -> Option<&str> {
        self.local_reason(command).or_else(|| reason(&self.machine_bound, command))
    }

    pub fn publish_ref(&self, head: &str) -> Option<String> {
        self.publish.as_ref().map(|prefix| format!("{prefix}{}", &head[..head.len().min(REF_SHORT)]))
    }

    fn malformed_pair(&self) -> Option<String> {
        let lists = [("land.local-only", &self.local_only), ("land.machine-bound", &self.machine_bound), ("land.after-only", &self.after_only), ("land.host-input", &self.host_inputs), ("land.deadline", &self.deadlines)];
        lists.into_iter().find_map(|(key, entries)| entries.iter().find(|entry| !entry.contains(SEPARATOR)).map(|entry| format!("invalid {key}: {entry} (expected REASON{SEPARATOR}COMMAND)")))
    }

    pub fn commands(&self, repo: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
        if let Some(refusal) = self.malformed_pair() {
            return Err(refusal);
        }
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
        status == SLOTS_BUSY_STATUS && offloaded && self.workflow && portable && !self.reserve
    }
}

fn inferred_checks(repo: &Path, head: &str) -> Vec<String> {
    let files = git_lines(repo, &["ls-tree", "--name-only", head]);
    let flake = files.iter().any(|file| file == "flake.nix").then(|| git_lines(repo, &["show", &format!("{head}:flake.nix")]).join("\n"));
    let package = files.iter().any(|file| file == "package.json").then(|| git_lines(repo, &["show", &format!("{head}:package.json")]).join("\n"));
    infer(&files, flake.as_deref(), package.as_deref())
}

fn reason<'a>(entries: &'a [String], command: &str) -> Option<&'a str> {
    let looked_up = super::shared_build::built(command);
    entries.iter().find_map(|entry| entry.split_once(SEPARATOR).filter(|(_, declared)| super::shared_build::built(declared) == looked_up).map(|(reason, _)| reason))
}

pub fn path_class(repo: &Path, base: &str, head: &str) -> String {
    let mut tops: Vec<String> = changed(repo, base, head, &[]).unwrap_or_default().iter().filter_map(|path| path.split('/').next()).map(str::to_owned).collect();
    tops.sort_unstable();
    tops.dedup();
    tops.join("+")
}

fn queued(entries: &[String]) -> impl Iterator<Item = &String> {
    entries.iter().filter(|entry| entry.split_once(SEPARATOR).is_some_and(|(patterns, _)| patterns.split(',').any(|spec| spec == ENQUEUE_PATTERN)))
}

fn touches(repo: &Path, base: &str, head: &str, patterns: &str) -> bool {
    let (rust, specs): (Vec<&str>, Vec<&str>) = patterns.split(',').filter(|spec| *spec != ENQUEUE_PATTERN).partition(|spec| *spec == RUST_INPUT);
    (!rust.is_empty() && changed(repo, base, head, &[]).map_or(true, |files| files.iter().any(|path| rust_build_input(path)))) || (!specs.is_empty() && changed(repo, base, head, &specs).map_or(true, |files| !files.is_empty()))
}

pub(super) fn changed(repo: &Path, base: &str, head: &str, specs: &[&str]) -> Result<Vec<String>, String> {
    let mut arguments: Vec<&str> = if base.is_empty() {
        vec!["ls-tree", "-r", "--name-only", head, "--"]
    } else {
        vec!["diff", "--name-only", "--no-renames", base, head, "--"]
    };
    arguments.extend(specs);
    git_output(repo, &arguments).map(|lines| lines.into_iter().filter(|line| !line.is_empty()).collect())
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
