use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::landing::{land_test_runner, land_test_selection};
use super::super::affected::Scope;
use super::super::it_modules::Selection;
use super::super::verdict_key;
use super::super::select::{Config, changed};
use super::super::selection_record::Recorded;
use super::arguments::{Arguments, Outcome};
use super::super::graph_selection::{Away, SELECTED, Selected};
use crate::platform::records_mirror::state_home;
use super::{say, warn};
use crate::platform::north_binary::land_north as program;

pub(super) fn selections(root: &Path, arguments: &Arguments, commands: &[String], recorded: Option<&Recorded>, runner: &str) -> (Option<Scope>, Option<String>) {
    let (it, packages) = it_for(root, arguments, commands, recorded, runner).unzip();
    let scope = scope_for(root, arguments, commands, recorded).map(|scope| scope.including(&packages.unwrap_or_default()));
    (scope, it)
}

fn scope_for(root: &Path, arguments: &Arguments, commands: &[String], recorded: Option<&Recorded>) -> Option<Scope> {
    if !verdict_key::reads_scope(commands) {
        return None;
    }
    if let Some(scope) = recorded.and_then(|recorded| recorded.scope.clone()) {
        say(&format!("safe-push: the after-landing Rust checks reuse the candidate's scope, {}", scope.arguments()));
        return Some(scope);
    }
    let scope = verdict_key::lane_scope(root, &arguments.base, &arguments.head, commands, |error| warn(&format!("safe-push: {error}; the Rust checks cover the whole workspace")))?;
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

fn it_for(root: &Path, arguments: &Arguments, commands: &[String], recorded: Option<&Recorded>, runner: &str) -> Option<(String, BTreeSet<String>)> {
    if !verdict_key::reads_it(commands) {
        return None;
    }
    if let Some(filters) = recorded.and_then(|recorded| recorded.it.clone()) {
        say(&format!("safe-push: the after-landing tests/it run reuses the candidate's selection, {filters}"));
        return Some((filters, BTreeSet::new()));
    }
    let program = program();
    let away = away(arguments, runner, &program);
    let selection = verdict_key::lane_it(root, &arguments.base, &arguments.head, &arguments.batch, away.as_ref(), commands, |error| warn(&format!("safe-push: {error}; the tests/it run covers every module")))?;
    if !arguments.leased {
        say(&format!("safe-push: the tests/it run is {}", selection.summary()));
    }
    let selected = selected(&selection);
    Outcome(arguments.outcome.clone()).record(&[SELECTED, &arguments.base, &arguments.head, &serde_json::to_string(&selected).unwrap_or_default()]);
    let since = if let Selection::Tests(tests) = &selection { tests.unselected_since() } else { Vec::new() };
    shadow_row(arguments, &selected, &since);
    Some((selection.arguments(), selection.packages()))
}

fn away<'a>(arguments: &'a Arguments, runner: &'a str, program: &'a str) -> Option<Away<'a>> {
    (!runner.is_empty() && runner != "build-host" && runner != north_base::platform::host::name()).then(|| Away { program: Path::new(program), host: runner, base: &arguments.base, tree: arguments.tree() })
}

fn selected(selection: &Selection) -> Selected {
    match selection {
        Selection::Every => Selected { every: true, ..Selected::default() },
        Selection::Modules { modules, units, floor } => Selected { tests: units.tests().map(str::to_owned).collect(), modules: modules.clone(), floor: floor.entries(), ..Selected::default() },
        Selection::Tests(tests) => tests.selected(),
    }
}

const GATE: &str = "test-selection";

fn shadow_row(arguments: &Arguments, selected: &Selected, since: &[String]) {
    let row = serde_json::json!({
        "at": crate::foundation::clock::Timestamp::now().unix_seconds_f64(),
        "base": arguments.base,
        "head": arguments.head,
        "after": arguments.after,
        "current": { "tests": selected.tests.len(), "added": selected.added.len(), "modules": selected.modules, "floor": selected.floor, "every": selected.every },
        "extra": { "since-map": since },
        "extra_tests": since.len(),
    });
    let directory = state_home().join("north/gates");
    let written = fs::create_dir_all(&directory).and_then(|()| OpenOptions::new().create(true).append(true).open(directory.join(format!("{GATE}.jsonl")))).and_then(|mut file| writeln!(file, "{row}"));
    if let Err(why) = written {
        warn(&format!("safe-push: the {GATE} gate's shadow row was not written: {why}"));
    }
}
