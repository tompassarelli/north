use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;

use super::mutant::{Env, Lease, Stop, git_bytes, git_text};
use super::mutant_trees::{self, RunDir};

const USAGE: &str = "usage: north land-fails-first [BASE HEAD] (default: $SAFE_PUSH_LAND_BASE $SAFE_PUSH_LAND_HEAD)";
const MARK: &str = "Behavior: unchanged";
const UNAVAILABLE: u8 = 75;

static HUNK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@").unwrap());
static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*#\[(?:[A-Za-z_][A-Za-z0-9_]*::)*test\b").unwrap());
static FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    Passes,
    Fails,
    Uncompiled,
    Absent,
}

enum Base {
    Verdicts(BTreeMap<String, Verdict>),
    Unverified(String),
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Passes => "passes",
            Verdict::Fails => "fails",
            Verdict::Uncompiled => "does not compile",
            Verdict::Absent => "does not exist",
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let pair = match arguments {
        [base, head] => Some((base.clone(), head.clone())),
        [] => std::env::var("SAFE_PUSH_LAND_BASE").ok().zip(std::env::var("SAFE_PUSH_LAND_HEAD").ok()),
        _ => None,
    };
    let Some((base, head)) = pair.filter(|(base, head)| !base.is_empty() && !head.is_empty()) else {
        eprintln!("{USAGE}");
        return UNAVAILABLE;
    };
    let env = Env { lease: Lease::Inherited, ..super::mutant_cli::process_env() };
    let outcome = std::env::current_dir().map_err(Stop::from).and_then(|cwd| check(&cwd, &base, &head, &env, &mut io::stdout()));
    match outcome {
        Ok(()) => 0,
        Err(Stop::Refused) => 1,
        Err(Stop::Usage(message) | Stop::Fail(message)) => {
            eprintln!("fails-first: could not run, not the lane's fault: {message}");
            UNAVAILABLE
        }
    }
}

fn is_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.split('/').rev().skip(1).any(|part| part == "tests") || name.ends_with("_tests.rs") || name.ends_with("_test.rs")
}

fn is_code(path: &str) -> bool {
    (path.starts_with("crates/") || path.starts_with("src/")) && !is_test(path)
}

pub(super) fn check(cwd: &Path, base: &str, head: &str, env: &Env, out: &mut dyn Write) -> Result<(), Stop> {
    let commit = |rev: &str| git_text(cwd, None, &["rev-parse", "--verify", "--end-of-options", &format!("{rev}^{{commit}}")]);
    let (base, head) = (commit(base)?, commit(head)?);
    let changed = git_bytes(cwd, None, &["diff", "--name-only", "--no-renames", "-z", &base, &head])?;
    let changed: Vec<String> = changed.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| String::from_utf8_lossy(path).into_owned()).collect();
    let tests: Vec<&str> = changed.iter().map(String::as_str).filter(|path| is_test(path)).collect();
    let mut names = BTreeSet::new();
    for path in tests.iter().filter(|path| path.ends_with(".rs")) {
        names.extend(named(cwd, &base, &head, path)?);
    }
    if names.is_empty() {
        writeln!(out, "fails-first: not applicable (the changed tests add or change no #[test] or prop_ function)")?;
        return Ok(());
    }
    if changed.iter().any(|path| is_code(path)) {
        against_base(cwd, &base, &head, &tests, &names, env, out)?;
    } else {
        writeln!(out, "fails-first: base runs not applicable (the lane does not change both code and tests)")?;
    }
    super::flake::check(cwd, &base, &head, &names, out)
}

fn against_base(cwd: &Path, base: &str, head: &str, tests: &[&str], names: &BTreeSet<String>, env: &Env, out: &mut dyn Write) -> Result<(), Stop> {
    let message = git_text(cwd, None, &["log", "-1", "--format=%B", head])?;
    let unchanged = message.lines().any(|line| line.trim() == MARK);
    let verdicts = match on_base(cwd, base, head, tests, names, env, out)? {
        Base::Verdicts(verdicts) => verdicts,
        Base::Unverified(reason) => return unverified(out, &reason),
    };
    for (name, verdict) in &verdicts {
        writeln!(out, "fails-first: on base {name} {}", verdict.label())?;
    }
    let failing: Vec<&str> = verdicts.iter().filter(|(_, verdict)| **verdict != Verdict::Passes).map(|(name, _)| name.as_str()).collect();
    let passing: Vec<&str> = verdicts.iter().filter(|(_, verdict)| **verdict == Verdict::Passes).map(|(name, _)| name.as_str()).collect();
    if unchanged && verdicts.values().any(|verdict| *verdict == Verdict::Uncompiled) {
        return unverified(out, "the lane is marked Behavior: unchanged, but its changed tests do not compile on base, so base cannot run them");
    }
    if unchanged && !failing.is_empty() {
        writeln!(out, "safe-push: ✗ fails-first: the lane is marked {MARK}, but these tests fail on base, so the change alters behavior: {}; drop the mark or keep the behavior; NOT pushed.", failing.join(", "))?;
        return Err(Stop::Refused);
    }
    if !unchanged && failing.is_empty() {
        writeln!(out, "safe-push: ✗ fails-first: every added or changed test passes on base, so none guards the change: {}; add a test that fails without the change, or put a line `{MARK}` in the commit message of a behavior-preserving change; NOT pushed.", passing.join(", "))?;
        return Err(Stop::Refused);
    }
    if unchanged {
        writeln!(out, "fails-first: ✓ {MARK}: all {} named test(s) pass on base", passing.len())?;
    } else {
        writeln!(out, "fails-first: ✓ {} of {} named test(s) fail on base: {}", failing.len(), verdicts.len(), failing.join(", "))?;
    }
    Ok(())
}

fn unverified(out: &mut dyn Write, reason: &str) -> Result<(), Stop> {
    writeln!(out, "fails-first: unverified ({reason})")?;
    Ok(())
}

fn overlay(cwd: &Path, head: &str, tests: &[&str], tree: &Path) -> Result<Option<String>, Stop> {
    for path in tests {
        let target = tree.join(path);
        let output = super::mutant::git(cwd, None, &["cat-file", "blob", &format!("{head}:{path}")])?;
        let placed = if output.status.success() {
            target.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&target, &output.stdout))
        } else {
            fs::remove_file(&target).or_else(|error| if error.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(error) })
        };
        if let Err(error) = placed {
            if !matches!(error.kind(), io::ErrorKind::NotADirectory | io::ErrorKind::IsADirectory | io::ErrorKind::AlreadyExists) {
                return Err(Stop::from(error));
            }
            return Ok(Some(format!("the lane's test file {path} cannot be placed on base: {error}")));
        }
    }
    Ok(None)
}

fn named(cwd: &Path, base: &str, head: &str, path: &str) -> Result<BTreeSet<String>, Stop> {
    let diff = String::from_utf8_lossy(&git_bytes(cwd, None, &["diff", "--no-color", "--no-ext-diff", "--unified=0", "--no-renames", base, head, "--", path])?).into_owned();
    let mut touched = BTreeSet::new();
    for found in diff.lines().filter_map(|line| HUNK.captures(line)) {
        let start: usize = found[1].parse().unwrap_or(0);
        match found.get(2).map(|count| count.as_str().parse::<usize>().unwrap_or(0)) {
            Some(0) => touched.extend([start, start + 1]),
            count => touched.extend(start..start + count.unwrap_or(1)),
        }
    }
    let output = super::mutant::git(cwd, None, &["show", &format!("{head}:{path}")])?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let mut starts = Vec::new();
    let mut attribute = None;
    for (index, line) in lines.iter().enumerate() {
        if ATTRIBUTE.is_match(line) {
            attribute.get_or_insert(index);
        } else if let Some(found) = FUNCTION.captures(line) {
            let name = found[1].to_owned();
            if attribute.is_some() || name.starts_with("prop_") {
                starts.push((attribute.unwrap_or(index), name));
            }
            attribute = None;
        } else if !line.trim_start().starts_with("#[") && !line.trim().is_empty() && !line.trim_start().starts_with("//") {
            attribute = None;
        }
    }
    let mut names = BTreeSet::new();
    for (position, (start, name)) in starts.iter().enumerate() {
        let end = starts.get(position + 1).map_or(lines.len(), |next| next.0);
        if touched.range(start + 1..=end).next().is_some() {
            names.insert(name.clone());
        }
    }
    Ok(names)
}

fn on_base(
    cwd: &Path,
    base: &str,
    head: &str,
    tests: &[&str],
    names: &BTreeSet<String>,
    env: &Env,
    out: &mut dyn Write,
) -> Result<Base, Stop> {
    let Some(config) = super::mutant_verify::config(cwd, base)? else {
        return Err(Stop::Fail(format!("{base}:.safe-push sets no mutant.run test runner")));
    };
    let top = PathBuf::from(git_text(cwd, None, &["rev-parse", "--show-toplevel"])?);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_nanos()).unwrap_or_default();
    let dir = RunDir(env.cache_home.join("north/fails-first").join(format!("{}-{nanos}", std::process::id())));
    fs::create_dir_all(&dir.0)?;
    let tree = dir.0.join("base");
    mutant_trees::checkout(&top, &dir.0.join("index"), base, None, &tree)?;
    if let Some(reason) = overlay(cwd, head, tests, &tree)? {
        return Ok(Base::Unverified(reason));
    }
    let shared = dir.0.join("links");
    let links = mutant_trees::share(&top, &shared, &config.links)?;
    mutant_trees::link(&shared, &tree, &links)?;
    let _ = fs::remove_file(tree.join(mutant_trees::RESULTS));
    let log = dir.0.join("runner.log");
    writeln!(out, "fails-first: running {} test(s) on base {} with the lane's tests", names.len(), &base[..base.len().min(12)])?;
    let seconds = mutant_trees::invoke(&config, env, "fails-first", std::slice::from_ref(&tree), names, &log)?;
    let results = mutant_trees::results(&tree);
    let text = fs::read_to_string(&log).unwrap_or_default();
    writeln!(out, "fails-first: runner finished in {seconds:.1} s")?;
    let missing = if !results.is_empty() || text.contains("test result:") {
        Verdict::Absent
    } else if text.contains("could not compile") {
        Verdict::Uncompiled
    } else {
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(40)..].join("\n");
        return Err(Stop::Fail(format!("the test runner reported no results; its output ends:\n{tail}")));
    };
    Ok(Base::Verdicts(names
        .iter()
        .map(|name| {
            let verdict = match results.get(name) {
                Some(true) => Verdict::Passes,
                Some(false) => Verdict::Fails,
                None => missing,
            };
            (name.clone(), verdict)
        })
        .collect()))
}

#[cfg(test)]
#[path = "fails_first_tests.rs"]
mod tests;
