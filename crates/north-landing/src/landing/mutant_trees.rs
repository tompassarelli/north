use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::mutant::{Config, Env, Lease, Stop, git, git_text};

const RESULTS: &str = ".mutant-results";
const TAIL_LINES: usize = 40;

pub(super) type Results = BTreeMap<String, bool>;

pub(super) struct Outcome {
    pub(super) change: Results,
    pub(super) mutants: BTreeMap<String, Results>,
    pub(super) seconds: f64,
    pub(super) tail: String,
}

struct RunDir(PathBuf);

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Plan<'a> {
    pub(super) head: &'a str,
    pub(super) mutants: BTreeMap<String, PathBuf>,
    pub(super) tests: BTreeSet<String>,
    pub(super) config: &'a Config,
    pub(super) repo: &'a str,
}

pub(super) fn run(cwd: &Path, plan: &Plan, env: &Env, out: &mut dyn Write) -> Result<Outcome, Stop> {
    let Plan { head, mutants, tests, config, repo } = plan;
    let top = PathBuf::from(git_text(cwd, None, &["rev-parse", "--show-toplevel"])?);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_nanos()).unwrap_or_default();
    let dir = RunDir(env.cache_home.join("north/mutant").join(repo).join(format!("{}-{nanos}", std::process::id())));
    fs::create_dir_all(&dir.0)?;
    let index = dir.0.join("index");
    let change = dir.0.join("change");
    checkout(&top, &index, head, None, &change)?;
    let mut trees = vec![change.clone()];
    let mut refused = false;
    for (id, patch) in mutants {
        let tree = dir.0.join("mutants").join(id);
        if checkout(&top, &index, head, Some(patch), &tree)? {
            trees.push(tree);
        } else {
            writeln!(out, "safe-push: ✗ mutant {id} does not apply to the change at HEAD; re-record it against the lane; NOT pushed.")?;
            refused = true;
        }
    }
    if refused {
        return Err(Stop::Refused);
    }
    let shared = dir.0.join("links");
    let links = share(&top, &shared, &config.links)?;
    for tree in &trees {
        link(&shared, tree, &links)?;
        let _ = fs::remove_file(tree.join(RESULTS));
    }
    let log = dir.0.join("runner.log");
    writeln!(out, "mutant: running {} test(s) over {} tree(s)", tests.len(), trees.len())?;
    let seconds = invoke(config, env, repo, &trees, tests, &log)?;
    let text = fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    let mutants = mutants.keys().map(|id| (id.clone(), results(&dir.0.join("mutants").join(id)))).collect();
    Ok(Outcome { change: results(&change), mutants, seconds, tail })
}

fn checkout(top: &Path, index: &Path, head: &str, patch: Option<&Path>, tree: &Path) -> Result<bool, Stop> {
    let _ = fs::remove_file(index);
    git_text(top, Some(index), &["read-tree", head])?;
    if let Some(patch) = patch {
        let patch = patch.to_string_lossy();
        let applied = |args: &[&str]| git(top, Some(index), args).is_ok_and(|output| output.status.success());
        if !applied(&["apply", "--cached", &patch]) && !applied(&["apply", "--cached", "--3way", &patch]) {
            return Ok(false);
        }
    }
    fs::create_dir_all(tree)?;
    git_text(top, Some(index), &["checkout-index", "-a", &format!("--prefix={}/", tree.display())])?;
    Ok(true)
}

fn copy(flags: &[&str], source: &Path, target: &Path) -> Result<(), Stop> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = Command::new("cp").args(flags).arg(source).arg(target).stdin(Stdio::null()).output()?;
    if output.status.success() {
        return Ok(());
    }
    let _ = fs::remove_dir_all(target).or_else(|_| fs::remove_file(target));
    Err(Stop::Fail(format!("mutant: cannot copy {}: {}", source.display(), String::from_utf8_lossy(&output.stderr).trim())))
}

fn share(top: &Path, shared: &Path, links: &[String]) -> Result<Vec<String>, Stop> {
    let mut present = Vec::new();
    for path in links {
        let source = top.join(path);
        if fs::symlink_metadata(&source).is_ok() {
            copy(&["-a", "--reflink=auto"], &source, &shared.join(path))?;
            present.push(path.clone());
        }
    }
    Ok(present)
}

fn link(shared: &Path, tree: &Path, links: &[String]) -> Result<(), Stop> {
    for path in links {
        let target = tree.join(path);
        if fs::symlink_metadata(&target).is_err() {
            copy(&["-al"], &shared.join(path), &target).or_else(|_| copy(&["-a", "--reflink=auto"], &shared.join(path), &target))?;
        }
    }
    Ok(())
}

fn invoke(config: &Config, env: &Env, repo: &str, trees: &[PathBuf], tests: &BTreeSet<String>, log: &Path) -> Result<f64, Stop> {
    let mut command = match &env.lease {
        Lease::Inherited => Command::new("sh"),
        Lease::Helper(helper) => {
            let mut command = Command::new("bun");
            command.arg(helper).args(["run", "--class", &config.class, "--owner", &format!("mutant:{repo}")]);
            command.args(["--timeout-seconds", "1800", "--", "sh"]);
            command
        }
        Lease::Missing(candidates) => {
            let shown: Vec<String> = candidates.iter().map(|path| path.display().to_string()).collect();
            return Err(Stop::Fail(format!("mutant: no machine-capacity helper at {}; NOT pushed.", shown.join(" or "))));
        }
    };
    let file = File::create(log)?;
    let joined: Vec<String> = trees.iter().map(|tree| tree.display().to_string()).collect();
    command
        .args(["-c", &config.run])
        .current_dir(&trees[0])
        .env("MUTANT_TESTS", tests.iter().cloned().collect::<Vec<_>>().join("\n"))
        .env("MUTANT_TREES", joined.join("\n"))
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file);
    let started = Instant::now();
    command.status()?;
    Ok(started.elapsed().as_secs_f64())
}

fn results(tree: &Path) -> Results {
    let mut verdicts = Results::new();
    for line in fs::read_to_string(tree.join(RESULTS)).unwrap_or_default().lines() {
        match line.split_once('\t') {
            Some(("pass", name)) => {
                verdicts.entry(name.to_owned()).or_insert(true);
            }
            Some(("fail", name)) => {
                verdicts.insert(name.to_owned(), false);
            }
            _ => {}
        }
    }
    verdicts
}
