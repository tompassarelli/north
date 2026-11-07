use std::path::Path;

use super::super::affected::{self, CHANGED_ENV, PACKAGES_ENV, Scope};
use super::super::it_modules::{self, IT_ENV};
use super::super::select::Config;
use super::arguments::Arguments;
use super::{say, warn};

pub(super) fn scope_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<Scope> {
    if !commands.iter().any(|command| command.contains(PACKAGES_ENV) || command.contains(CHANGED_ENV)) {
        return None;
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

pub(super) fn it_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<String> {
    if !commands.iter().any(|command| command.contains(IT_ENV)) {
        return None;
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
