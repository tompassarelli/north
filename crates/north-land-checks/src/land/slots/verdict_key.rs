use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use north_base::verdict_cache::{self, Check, Pass};

use super::affected::{self, CHANGED_ENV, CPUS_ENV, PACKAGES_ENV, Scope};
use super::graph_selection::Away;
use super::it_modules::{self, IT_ENV, Stage};
use crate::landing::land_test_selection::SELECT_PATHS_ENV;

pub fn key(root: &Path, base: &str, tree: &str, command: &str, scope_environment: &[(String, String)]) -> Option<String> {
    let mut environment: Vec<(String, String)> = [("SAFE_PUSH_LAND_BASE", base), ("SAFE_PUSH_LAND_HEAD", tree), ("SAFE_PUSH_LAND_RUNTIME_HEAD", tree)].into_iter().map(|(name, value)| (name.to_owned(), value.to_owned())).collect();
    environment.extend(scope_environment.iter().cloned());
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let (south, nixos) = (north_base::platform::shared_config::operator_root(&home), home.join("code/nixos-config/main"));
    let machine_repos: [(&str, &Path); 2] = [("south", &south), ("nixos-config", &nixos)];
    verdict_cache::key(&Check { repo: root, tree, command, runtime: &verdict_cache::runtime_identity(), environment: &environment, machine_repos: &machine_repos })
}

pub fn selection_environment(scope: Option<&Scope>, it: Option<&str>, selected_paths: Option<&str>) -> Vec<(&'static str, String)> {
    let mut environment = Vec::new();
    if let Some(scope) = scope {
        environment.push((PACKAGES_ENV, scope.arguments()));
        environment.push((CHANGED_ENV, scope.changed_arguments()));
        environment.extend(scope.cpus().map(|cpus| (CPUS_ENV, cpus.to_string())));
    }
    environment.extend(it.map(|filters| (IT_ENV, filters.to_owned())));
    environment.extend(selected_paths.map(|paths| (SELECT_PATHS_ENV, paths.to_owned())));
    environment
}

fn reads(commands: &[String], name: &str) -> bool {
    commands.iter().any(|command| command.contains(name))
}

pub fn reads_scope(commands: &[String]) -> bool {
    reads(commands, PACKAGES_ENV) || reads(commands, CHANGED_ENV)
}

pub fn reads_it(commands: &[String]) -> bool {
    reads(commands, IT_ENV)
}

pub fn lane_scope(root: &Path, base: &str, head: &str, commands: &[String], on_error: impl FnOnce(String)) -> Option<Scope> {
    reads_scope(commands).then(|| {
        affected::scope(root, base, head).unwrap_or_else(|error| {
            on_error(error);
            Scope::Workspace
        })
    })
}

pub fn lane_it(root: &Path, base: &str, head: &str, stage: Stage<'_>, away: Option<&Away<'_>>, commands: &[String], on_error: impl FnOnce(String)) -> Option<it_modules::Selection> {
    reads_it(commands).then(|| {
        it_modules::selection(root, base, head, stage, away).unwrap_or_else(|error| {
            on_error(error);
            it_modules::Selection::Every
        })
    })
}

pub fn lane_environment(root: &Path, base: &str, head: &str, away: Option<&Away<'_>>, commands: &[String]) -> Vec<(String, String)> {
    let it = lane_it(root, base, head, Stage::BeforePush, away, commands, |_| {});
    let packages = it.as_ref().map(it_modules::Selection::packages).unwrap_or_default();
    let scope = lane_scope(root, base, head, commands, |_| {}).map(|scope| scope.including(&packages));
    let it = it.map(|selection| selection.arguments());
    selection_environment(scope.as_ref(), it.as_deref(), None).into_iter().map(|(name, value)| (name.to_owned(), value)).collect()
}

pub fn hit_arguments(keys: &[&str]) -> Vec<String> {
    ["capacity", "offload", "verdict", "hit"].into_iter().chain(keys.iter().copied()).map(str::to_owned).collect()
}

pub fn record_arguments(host: &str, key: &str, passed_tree: Option<&str>) -> Vec<String> {
    let mut arguments: Vec<String> = ["capacity", "offload", "verdict", "record", "--host", host, "--key", key].map(str::to_owned).into();
    match passed_tree {
        Some(tree) => arguments.extend(["--pass".to_owned(), tree.to_owned()]),
        None => arguments.push("--fail".to_owned()),
    }
    arguments
}

pub fn hits(text: &str) -> BTreeMap<String, (String, Pass)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            let (key, host, seconds, sha) = (words.next()?, words.next()?, words.next()?, words.next()?);
            Some((key.to_owned(), (host.to_owned(), Pass::parse(&format!("{seconds} {sha}"))?)))
        })
        .collect()
}
