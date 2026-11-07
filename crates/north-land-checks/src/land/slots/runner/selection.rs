use std::path::Path;
use std::process::Command;

use crate::landing::{land_test_runner, land_test_selection};
use super::super::affected::{self, CHANGED_ENV, PACKAGES_ENV, Scope};
use super::super::it_modules::{self, IT_ENV};
use super::super::select::{Config, changed};
use super::super::selection_record::Recorded;
use super::arguments::Arguments;
use super::{say, warn};

pub(super) fn scope_for(root: &Path, arguments: &Arguments, commands: &[String], recorded: Option<&Recorded>) -> Option<Scope> {
    if !commands.iter().any(|command| command.contains(PACKAGES_ENV) || command.contains(CHANGED_ENV)) {
        return None;
    }
    if let Some(scope) = recorded.and_then(|recorded| recorded.scope.clone()) {
        say(&format!("safe-push: the after-landing Rust checks reuse the candidate's scope, {}", scope.arguments()));
        return Some(scope);
    }
    let scope = affected::scope(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the Rust checks cover the whole workspace"));
        Scope::Workspace
    });
    if !arguments.leased {
        say(&format!("safe-push: the Rust checks cover {}", scope.arguments()));
    }
    Some(scope)
}

pub(super) fn defined_on_tree(root: &Path, arguments: &Arguments, commands: Vec<String>) -> Vec<String> {
    if !arguments.pinned() {
        return commands;
    }
    let tree = Config::load(root, arguments.tree());
    let tree = if arguments.after { tree.after_set() } else { tree };
    commands.into_iter().filter(|command| tree.runs(command)).collect()
}

pub(super) fn without_empty_test_selections(root: &Path, arguments: &Arguments, commands: Vec<String>) -> Vec<String> {
    if arguments.base.is_empty() || !commands.iter().any(|command| land_test_runner::selection_of(command).is_some()) || !checked_out(root, arguments.tree()) {
        return commands;
    }
    let changes = changed(root, &arguments.base, &arguments.head, &[]);
    commands
        .into_iter()
        .filter(|command| {
            let Some((scope, only)) = land_test_runner::selection_of(command) else { return true };
            let empty = land_test_selection::select_among(root, scope, &changes, only).is_ok_and(|selected| selected.is_empty());
            if empty {
                say(&format!("safe-push: land check {command} selects no test in this change, so it passes without a slot"));
            }
            !empty
        })
        .collect()
}

fn checked_out(root: &Path, tree: &str) -> bool {
    let commit = |revision: &str| {
        Command::new("git").arg("-C").arg(root).args(["rev-parse", "--verify", "--quiet", &format!("{revision}^{{commit}}")]).output().ok().filter(|output| output.status.success()).map(|output| output.stdout)
    };
    let clean = Command::new("git").arg("-C").arg(root).args(["status", "--porcelain", "--untracked-files=no"]).output().is_ok_and(|output| output.status.success() && output.stdout.is_empty());
    clean && commit("HEAD").is_some_and(|head| Some(head) == commit(tree))
}

pub(super) fn it_for(root: &Path, arguments: &Arguments, commands: &[String], recorded: Option<&Recorded>) -> Option<String> {
    if !commands.iter().any(|command| command.contains(IT_ENV)) {
        return None;
    }
    if let Some(filters) = recorded.and_then(|recorded| recorded.it.clone()) {
        say(&format!("safe-push: the after-landing tests/it run reuses the candidate's selection, {filters}"));
        return Some(filters);
    }
    let selection = it_modules::selection(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the tests/it run covers every module"));
        it_modules::Selection::Every
    });
    if !arguments.leased {
        say(&format!("safe-push: the tests/it run is {}", selection.arguments()));
    }
    Some(selection.arguments())
}
