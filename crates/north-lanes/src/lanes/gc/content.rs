use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::exec::{git, real};
use super::state::write_atomically;

const CACHE: &str = "north/lane-gc-patch-ids";

pub(super) struct MainContent {
    patches: HashSet<String>,
    trees: HashSet<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    diffed: usize,
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

fn cache_path(repo: &Path) -> Option<PathBuf> {
    let common = git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    common.ok.then(|| real(Path::new(&common.out)).join(CACHE))
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

fn main_patches(repo: &Path, base: &str) -> (HashMap<String, String>, usize) {
    let path = cache_path(repo);
    let mut known = path.as_deref().map(cached).unwrap_or_default();
    let listed = git(repo, &["rev-list", base]);
    let commits: Vec<&str> = listed.out.lines().filter(|line| !line.is_empty()).collect();
    if !listed.ok || commits.is_empty() {
        return (HashMap::new(), 0);
    }
    let wanted: HashSet<&str> = commits.iter().copied().collect();
    let on_main = |known: HashMap<String, String>| known.into_iter().filter(|(sha, _)| wanted.contains(sha.as_str())).collect::<HashMap<_, _>>();
    let missing: Vec<&str> = commits.iter().copied().filter(|sha| !known.contains_key(*sha)).collect();
    if missing.is_empty() {
        return (on_main(known), 0);
    }
    let fresh = if missing.len() == commits.len() { patch_ids(repo, &[base]) } else { patch_ids_of(repo, &missing) };
    let Some(fresh) = fresh else {
        return (on_main(known), 0);
    };
    let diffed = fresh.len();
    for (sha, id) in fresh {
        known.insert(sha, id);
    }
    if let Some(path) = path.as_deref() {
        store(path, &known);
    }
    (on_main(known), diffed)
}

impl MainContent {
    pub(super) fn read(repo: &Path, base: &str) -> Self {
        let trees = git(repo, &["log", "--format=%T", base]);
        let (patches, diffed) = main_patches(repo, base);
        Self {
            patches: patches.into_values().collect(),
            trees: if trees.ok { trees.out.lines().map(str::to_owned).collect() } else { HashSet::new() },
            diffed,
        }
    }

    pub(super) fn holds(&self, repo: &Path, base: &str, tip: &str) -> Option<&'static str> {
        let tree = git(repo, &["rev-parse", &format!("{tip}^{{tree}}")]);
        if tree.ok && self.trees.contains(&tree.out) {
            return Some("tree on main");
        }
        let lane = patch_ids(repo, &[tip, &format!("^{base}")])?;
        (!lane.is_empty() && lane.iter().all(|(_, patch)| self.patches.contains(patch))).then_some("patches on main")
    }

    #[cfg(test)]
    pub(super) fn diffed(&self) -> usize {
        self.diffed
    }
}
