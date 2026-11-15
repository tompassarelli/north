use std::collections::BTreeSet;

use crate::landing::check_findings;
use north_foundation::foundation::check_log::{github_rate_limit, ref_lock, stale_token};
use north_foundation::foundation::exit::TRANSPORT_FAILED_TEXT;
use crate::landing::red_check::{Failure, failures, log_text};

const RUNNER_CHATTER: [&str; 4] = ["head-north: ", "safe-push: ", "north capacity offload: ", "land-shell: "];
const STOPPED_EARLY: [&str; 4] = ["were not run due to test failure", "Cancelling due to test failure", "could not compile", "build failed, waiting for other jobs"];
const PATH_EDGES: &[char] = &['`', '\'', '"', ':', ',', ';', '(', ')', '[', ']'];
const UNRUN: [&str; 3] = ["failed to download", "command not found", "No space left on device"];
const BUCK2_CLEAN: [&str; 4] = ["Timeout 0", "Fatal 0", "Infra Failure 0", "Build failure 0"];
const ROOT_PACKAGE: [&str; 3] = ["src/", "tests/", "build.rs"];
const WORKSPACE: [&str; 2] = ["Cargo.toml", "Cargo.lock"];

/// Whose failure a land check's red run is, judged from the lane's log against its base's failures.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Attribution {
    /// The lane adds these failures, which the base does not have.
    Lane(Vec<String>),
    /// The lane changes these files of the failing check or test, so the failure may be its own.
    Touches(Vec<String>),
    /// The lane's log cannot be compared with its base's.
    Unjudged(&'static str),
    /// Every failure of the lane is the base's, and the lane changes none of the failing check's own files.
    Base,
}

/// The lane's red run of one check, as attribution reads it.
pub(super) struct LaneRun<'a> {
    pub(super) command: &'a str,
    pub(super) log: &'a str,
    pub(super) changed: &'a [String],
    /// Whether the check runs tests, whose failures it names one by one.
    pub(super) tests: bool,
    /// Whether the check reports every error it finds: the base's .safe-push declares it land.reports-all, or the repository
    /// declares no check and an inferred one, which no lane is held to, gates only on the findings it adds.
    pub(super) reports_all: bool,
}

/// What attribution reads of the repository: which repository paths exist, which files define a test, and which packages a
/// package depends on by path (each a directory with a trailing slash, the root package ""), `None` when its manifest cannot be read.
pub(super) trait Repository {
    fn exists(&self, path: &str) -> bool;
    fn test_files(&self, name: &str) -> Vec<String>;
    fn path_dependencies(&self, package: &str) -> Option<Vec<String>>;
}

/// The failures a check log names: its failing tests, harness checks and compiler diagnostics, and for a check that runs no
/// tests every line it printed besides, each absolute path cut to the repository path that exists, so a lane worktree and
/// a base tree report the same failure alike and a lane line the base never printed reads as new.
pub(super) fn signature(log: &str, tests: bool, repository: &dyn Repository) -> BTreeSet<String> {
    let mut found = check_findings::findings(log);
    if !tests {
        found.extend(log.lines().map(log_text).filter(|line| !line.is_empty() && !RUNNER_CHATTER.iter().any(|chatter| line.starts_with(chatter))).map(str::to_owned));
    }
    found.iter().map(|line| relative(line, repository)).collect()
}

fn relative(line: &str, repository: &dyn Repository) -> String {
    line.split(' ').map(|word| repository_path(word, repository).unwrap_or_else(|| word.to_owned())).collect::<Vec<_>>().join(" ")
}

fn repository_path(word: &str, repository: &dyn Repository) -> Option<String> {
    let path = word.trim_matches(PATH_EDGES);
    let parts: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let inner = (1..parts.len()).map(|from| parts[from..].join("/")).find(|inner| repository.exists(inner))?;
    Some(word.replacen(path, &inner, 1))
}

/// Whether a test run or a build stopped at its first failure, so a failure the base shares may hide a test or a crate the lane breaks.
pub(super) fn stopped_early(log: &str) -> bool {
    STOPPED_EARLY.iter().any(|marker| log.contains(marker))
}

/// Whether a test run reached its end with every test it selected run: a nextest summary that counts no test left unrun, or a
/// Buck2 test summary with no timeout, fatal binary, infrastructure or build failure.
fn completed(log: &str) -> bool {
    let mut finished = false;
    for line in log.lines().map(log_text) {
        match summary(line) {
            Some(false) => return false,
            Some(true) => finished = true,
            None => {}
        }
    }
    finished
}

/// Whether a summary line counts every selected test run: `Some(false)` for a nextest count of N/M or a Buck2 summary with a
/// timeout, fatal binary, infrastructure or build failure, `None` for a line that is no summary.
fn summary(line: &str) -> Option<bool> {
    if let Some((_, counted)) = line.strip_prefix("Summary [").and_then(|rest| rest.split_once(']')) && (counted.contains(" tests run:") || counted.contains(" test run:")) {
        return Some(counted.split_whitespace().next().is_some_and(|count| !count.contains('/')));
    }
    line.starts_with("Tests finished:").then(|| BUCK2_CLEAN.iter().all(|clean| line.contains(clean)))
}

/// Why the check could not judge the tree at all, as a host, a fetch or a credential fails it the same way on any tree.
fn environment_failure(log: &str) -> Option<&'static str> {
    if UNRUN.iter().any(|marker| log.contains(marker)) || log.contains(TRANSPORT_FAILED_TEXT) {
        return Some("the check failed in its environment, before it judged the tree");
    }
    (ref_lock(log).is_some() || github_rate_limit(log).is_some() || stale_token(log).is_some()).then_some("the check failed on a git lock, a rate limit or a stale token, before it judged the tree")
}

/// The failing check's own files: the code its `--code` list names, every repository path its failures name, and for its
/// failing tests their defining files, their packages and every package those depend on by path, with the workspace manifest
/// and lock, since a lane that breaks the code a red test exercises fails that same test. A failure whose code cannot be told
/// leaves the failure unjudged.
fn own_files(command: &str, found: &BTreeSet<String>, log: &str, repository: &dyn Repository) -> Result<BTreeSet<String>, &'static str> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let code = words.windows(2).filter(|pair| pair[0] == "--code").flat_map(|pair| pair[1].split(','));
    let named = found.iter().flat_map(|finding| finding.split_whitespace()).map(|word| word.trim_matches(PATH_EDGES)).filter(|word| word.contains('/') && !word.starts_with('/') && !word.contains("://"));
    let mut own: BTreeSet<String> = code.chain(named).map(str::to_owned).collect();
    let mut packages = BTreeSet::new();
    for failure in failures(log) {
        match failure {
            Failure::Cargo(name) => {
                let files = repository.test_files(&name);
                if files.is_empty() {
                    return Err("no file defines a failing test by its name, so the code it exercises is unknown");
                }
                packages.extend(files.iter().map(|file| package(file, repository)));
                own.extend(files);
            }
            Failure::Harness(path) => {
                own.insert(path);
            }
            Failure::Line(_) | Failure::Step(_) => return Err("a failure names no test or file, so the code it exercises is unknown"),
        }
    }
    if !packages.is_empty() {
        own.extend(WORKSPACE.map(str::to_owned));
    }
    for dir in dependency_closure(packages, repository)? {
        if dir.is_empty() {
            own.extend(ROOT_PACKAGE.map(str::to_owned));
        } else {
            own.insert(dir);
        }
    }
    own.retain(|path| !path.is_empty());
    Ok(own)
}

/// The directory of the package holding `file`, the nearest above it with a Cargo.toml, with a trailing slash; "" for the root package.
fn package(file: &str, repository: &dyn Repository) -> String {
    let parts: Vec<&str> = file.split('/').collect();
    (1..parts.len()).rev().map(|dirs| parts[..dirs].join("/")).find(|dir| repository.exists(&format!("{dir}/Cargo.toml"))).map_or_else(String::new, |dir| format!("{dir}/"))
}

fn dependency_closure(packages: BTreeSet<String>, repository: &dyn Repository) -> Result<BTreeSet<String>, &'static str> {
    let mut seen = BTreeSet::new();
    let mut next: Vec<String> = packages.into_iter().collect();
    while let Some(dir) = next.pop() {
        if seen.contains(&dir) {
            continue;
        }
        next.extend(repository.path_dependencies(&dir).ok_or("the base manifest of a package a failing test exercises cannot be read, so the code it exercises is unknown")?);
        seen.insert(dir);
    }
    Ok(seen)
}

fn touched(changed: &[String], own: &BTreeSet<String>) -> Vec<String> {
    changed.iter().filter(|path| own.iter().any(|file| *path == file || path.starts_with(&format!("{}/", file.trim_end_matches('/'))))).cloned().collect()
}

/// Judges the lane's red run against `base`, the failures the same check printed on the lane's base tree.
pub(super) fn attribute(run: &LaneRun, base: &BTreeSet<String>, repository: &dyn Repository) -> Attribution {
    if stopped_early(run.log) {
        return Attribution::Unjudged("the lane's run stopped at its first failure, so a test or crate the lane breaks may not have run or compiled");
    }
    if let Some(line) = environment_failure(run.log) {
        return Attribution::Unjudged(line);
    }
    if run.tests && !completed(run.log) {
        return Attribution::Unjudged("the lane's test run shows no summary of every test it selected, so a test the lane breaks may not have run");
    }
    if !run.tests && !run.reports_all {
        return Attribution::Unjudged("the check is not declared to report every error it finds (land.reports-all), so main's first error may hide one the lane adds");
    }
    let lane = signature(run.log, run.tests, repository);
    if lane.is_empty() {
        return Attribution::Unjudged("the lane's log names no failure to compare");
    }
    let base: BTreeSet<String> = base.iter().map(|finding| relative(finding, repository)).collect();
    let added = check_findings::added(&lane, &base);
    if !added.is_empty() {
        return Attribution::Lane(added);
    }
    let own = match own_files(run.command, &lane, run.log, repository) {
        Ok(own) => own,
        Err(why) => return Attribution::Unjudged(why),
    };
    let touches = touched(run.changed, &own);
    if touches.is_empty() { Attribution::Base } else { Attribution::Touches(touches) }
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
