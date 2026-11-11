use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::exec::{git, real};
use super::state::write_atomically;

const CACHE: &str = "north/lane-gc-patch-ids";

pub(super) struct MainContent {
    patches: HashSet<String>,
    trees: HashSet<String>,
    known: Mutex<Known>,
    diffed: AtomicUsize,
}

fn patch_ids(repo: &Path, selection: &[&str]) -> Option<Vec<(String, String)>> {
    let mut log = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repo)
        .args(["log", "--no-merges", "--no-ext-diff", "--no-color", "-p", "--format=commit %H"])
        .args(selection)
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut ids = Command::new("git").args(["patch-id", "--stable"]).stdin(log.stdout.take()?).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut text = String::new();
    ids.stdout.take()?.read_to_string(&mut text).ok()?;
    let (logged, identified) = (log.wait().ok()?, ids.wait().ok()?);
    (logged.success() && identified.success()).then(|| {
        text.lines()
            .filter_map(|line| {
                let (id, sha) = line.split_once(' ')?;
                Some((sha.to_owned(), id.to_owned()))
            })
            .collect()
    })
}

fn patch_ids_of(repo: &Path, commits: &[&str]) -> Option<Vec<(String, String)>> {
    let mut selection = vec!["--no-walk=unsorted"];
    selection.extend_from_slice(commits);
    patch_ids(repo, &selection)
}

pub(super) fn cache_path(repo: &Path, name: &str) -> Option<PathBuf> {
    let common = git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    common.ok.then(|| real(Path::new(&common.out)).join(name))
}

fn cached(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter_map(|line| {
                    let (sha, id) = line.split_once(' ')?;
                    Some((sha.to_owned(), id.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn store(path: &Path, ids: &HashMap<String, String>) {
    let mut rows: Vec<String> = ids.iter().map(|(sha, id)| format!("{sha} {id}")).collect();
    rows.sort();
    let _ = write_atomically(path, &format!("{}\n", rows.join("\n")));
}

#[derive(Default)]
struct Known {
    path: Option<PathBuf>,
    ids: HashMap<String, String>,
}

impl Known {
    fn load(repo: &Path) -> Self {
        let path = cache_path(repo, CACHE);
        let ids = path.as_deref().map(cached).unwrap_or_default();
        Self { path, ids }
    }

    fn missing<'a>(&self, commits: &[&'a str]) -> Vec<&'a str> {
        commits.iter().copied().filter(|sha| !self.ids.contains_key(*sha)).collect()
    }

    fn add(&mut self, fresh: Vec<(String, String)>) -> usize {
        let diffed = fresh.len();
        self.ids.extend(fresh);
        if let Some(path) = self.path.as_deref() {
            store(path, &self.ids);
        }
        diffed
    }
}

fn listed(repo: &Path, selection: &[&str]) -> Option<Vec<String>> {
    let mut arguments = vec!["rev-list"];
    arguments.extend_from_slice(selection);
    let listed = git(repo, &arguments);
    listed.ok.then(|| listed.out.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

fn diff_missing(repo: &Path, known: &Mutex<Known>, commits: &[&str], whole: &[&str]) -> Option<usize> {
    let missing = known.lock().ok()?.missing(commits);
    if missing.is_empty() {
        return Some(0);
    }
    let fresh = if missing.len() == commits.len() { patch_ids(repo, whole) } else { patch_ids_of(repo, &missing) }?;
    Some(known.lock().ok()?.add(fresh))
}

impl MainContent {
    pub(super) fn read(repo: &Path, base: &str) -> Self {
        let trees = git(repo, &["log", "--format=%T", base]);
        let known = Mutex::new(Known::load(repo));
        let commits = listed(repo, &[base]).unwrap_or_default();
        let commits: Vec<&str> = commits.iter().map(String::as_str).collect();
        let diffed = if commits.is_empty() { 0 } else { diff_missing(repo, &known, &commits, &[base]).unwrap_or(0) };
        let patches = known.lock().map(|known| commits.iter().filter_map(|sha| known.ids.get(*sha).cloned()).collect()).unwrap_or_default();
        Self {
            patches,
            trees: if trees.ok { trees.out.lines().map(str::to_owned).collect() } else { HashSet::new() },
            known,
            diffed: AtomicUsize::new(diffed),
        }
    }

    pub(super) fn holds(&self, repo: &Path, base: &str, tip: &str) -> Option<&'static str> {
        let tree = git(repo, &["rev-parse", &format!("{tip}^{{tree}}")]);
        if tree.ok && self.trees.contains(&tree.out) {
            return Some("tree on main");
        }
        let outside = format!("^{base}");
        let commits = listed(repo, &["--no-merges", tip, &outside])?;
        let commits: Vec<&str> = commits.iter().map(String::as_str).collect();
        self.diffed.fetch_add(diff_missing(repo, &self.known, &commits, &[tip, &outside])?, Ordering::Relaxed);
        let known = self.known.lock().ok()?;
        let lane: Vec<&String> = commits.iter().filter_map(|sha| known.ids.get(*sha)).collect();
        (!lane.is_empty() && lane.iter().all(|patch| self.patches.contains(*patch))).then_some("patches on main")
    }

    #[cfg(test)]
    pub(super) fn diffed(&self) -> usize {
        self.diffed.load(Ordering::Relaxed)
    }
}
