use std::path::{Path, PathBuf};
use std::process::Command;

use super::tools;
use super::verb_help::{self, Resolution};
use crate::landing::host_calls::{self, HostCall, Source};

const USAGE: &str = "usage: north check calls [--tree DIR] [--live DIR]";

pub(super) fn findings(calls: &[HostCall], resolve: impl Fn(&[String]) -> Resolution) -> Vec<String> {
    let mut found = Vec::new();
    for call in calls.iter().filter(|call| call.source != Source::Rust) {
        match (resolve(&call.words), call.source) {
            (Resolution::Command, _) | (Resolution::NoVerb(_), Source::Hop) => {}
            (Resolution::Unknown(reason) | Resolution::NoVerb(reason), _) => found.push(format!("{}: north {}: {reason}", call.location, call.words.join(" "))),
            (Resolution::Group, Source::Hop) => {
                for relayed in calls.iter().filter(|other| other.words.len() > call.words.len() && other.words.starts_with(&call.words)) {
                    if let Resolution::Unknown(reason) = resolve(&relayed.words) {
                        found.push(format!("{}: relays north {} from {}: {reason}", call.location, relayed.words.join(" "), relayed.location));
                    }
                }
            }
            (Resolution::Group, _) => {}
        }
    }
    found.dedup();
    found
}

fn summary(calls: &[HostCall]) -> String {
    let count = |source| calls.iter().filter(|call| call.source == source).count();
    format!("{} in units and the scripts they run, {} in hooks, {} in its ssh hops to other hosts", count(Source::Unit), count(Source::Hook), count(Source::Hop))
}

pub(super) fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [flag] if flag == "-h" || flag == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let (mut tree, mut live) = (None, None);
    for pair in arguments.chunks(2) {
        match pair {
            [flag, value] if flag == "--tree" => tree = Some(PathBuf::from(value)),
            [flag, value] if flag == "--live" => live = Some(PathBuf::from(value)),
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let live_table = match live.as_deref().map(live_reference).transpose() {
        Ok(table) => table.unwrap_or_default(),
        Err(why) => {
            println!("north check calls: ⚠ {why}, so only verbs were checked, not the subcommands it drops");
            String::new()
        }
    };
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let gathered = host_calls::on_this_host(&home, tree.as_deref());
    gathered.warnings.iter().for_each(|warning| println!("north check calls: ⚠ {warning}"));
    let found = findings(&gathered.calls, |words| verb_help::resolve(verb_help::reference(), &live_table, words, tools::defines));
    if found.is_empty() {
        let checked = gathered.calls.iter().filter(|call| call.source != Source::Rust).count();
        println!("north check calls: {checked} north calls on this host parse against this runtime ({})", summary(&gathered.calls));
        return 0;
    }
    found.iter().for_each(|line| println!("north check calls: ✗ {line}"));
    1
}

fn live_reference(runtime: &Path) -> Result<String, String> {
    let north = runtime.join("north");
    let output = Command::new(&north).arg("--help-all").output().map_err(|error| format!("cannot run {} --help-all: {error}", north.display()))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        false => Err(format!("{} --help-all exited {}", north.display(), output.status)),
    }
}

#[cfg(test)]
#[path = "call_check_tests.rs"]
mod tests;
