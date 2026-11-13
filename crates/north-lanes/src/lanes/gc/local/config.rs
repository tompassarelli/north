use std::collections::{HashMap, HashSet};

use super::super::Gc;
use super::super::exec::git;
use super::Container;

/// The names of the `branch.<name>` sections listed by `git config --local --get-regexp '^branch\.'`
/// whose branch is not in `branches`. A key is `branch.<name>.<variable>`, and a variable holds no dot,
/// so the name is everything before the last dot.
fn orphaned_branch_sections(listed: &str, branches: &HashSet<String>) -> Vec<String> {
    let mut names: Vec<String> = listed
        .lines()
        .filter_map(|line| line.split_whitespace().next()?.strip_prefix("branch."))
        .filter_map(|key| key.rsplit_once('.').map(|(name, _)| name.to_owned()))
        .filter(|name| !branches.contains(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The branches other than main whose listed upstream is origin's main: `north lane new` once wrote that
/// upstream into the shared config for every lane, and a lane needs none because safe-push pushes an
/// explicit refspec, so the keys only lengthen the config every concurrent lane creation rewrites.
fn lane_upstreams(listed: &str) -> Vec<String> {
    let mut remotes: HashMap<&str, &str> = HashMap::new();
    let mut merges: HashMap<&str, &str> = HashMap::new();
    for (key, value) in listed.lines().filter_map(|line| line.split_once(' ')) {
        let Some((name, variable)) = key.strip_prefix("branch.").and_then(|key| key.rsplit_once('.')) else { continue };
        match variable {
            "remote" => remotes.insert(name, value),
            "merge" => merges.insert(name, value),
            _ => None,
        };
    }
    let mut names: Vec<String> = remotes
        .into_iter()
        .filter(|(name, remote)| *name != "main" && *remote == "origin" && merges.get(name) == Some(&"refs/heads/main"))
        .map(|(name, _)| name.to_owned())
        .collect();
    names.sort();
    names
}

impl Gc {
    /// Removes the `branch.<name>` sections whose branch no longer exists and the origin/main upstream of
    /// every lane branch: `delete_branch` drops the ref with `git update-ref -d`, which leaves the section
    /// behind, and lanes created before north#1047 each carry an upstream, so a machine that has run whole
    /// fleets of lanes carries thousands of keys in the config every lane shares (north#1054).
    pub(super) fn prune_branch_config(&mut self, sweep: &Container) {
        let branches: HashSet<String> = git(&sweep.main, &["for-each-ref", "--format=%(refname:short)", "refs/heads"]).out.lines().map(str::to_owned).collect();
        let listed = git(&sweep.main, &["config", "--local", "--get-regexp", r"^branch\."]).out;
        let orphans = orphaned_branch_sections(&listed, &branches);
        let upstreams: Vec<String> = lane_upstreams(&listed).into_iter().filter(|name| branches.contains(name)).collect();
        if self.options.dry_run {
            if !orphans.is_empty() {
                println!("{}: prune {} branch configs whose branch no longer exists", sweep.name, orphans.len());
            }
            if !upstreams.is_empty() {
                println!("{}: drop the origin/main upstream of {} lane branches", sweep.name, upstreams.len());
            }
            return;
        }
        for name in orphans {
            if git(&sweep.main, &["config", "--local", "--remove-section", &format!("branch.{name}")]).ok {
                self.counts.pruned_branch_configs += 1;
            }
        }
        for name in upstreams {
            let dropped = ["remote", "merge"].map(|variable| git(&sweep.main, &["config", "--local", "--unset", &format!("branch.{name}.{variable}")]).ok);
            if dropped.iter().all(|ok| *ok) {
                self.counts.dropped_upstreams += 1;
            }
        }
    }
}
