pub use north_base::platform;
pub use north_diff_gates::{build_cache_gate, dep_gate, language_gate};
use north_diff_gates::language_gate::EMPTY_TREE;
pub use north_landing_gates::landing;

pub mod doc_links;
mod doc_paths;
pub mod docs_owners;
pub mod ci_budget;
pub mod dropped_commits;
pub mod detector_proof;
pub mod gate_shadow;
pub mod generated;
pub mod generated_registry;
pub mod git;
pub mod holdlist;
pub(crate) mod host_paths;
pub mod incident_guard;
pub mod language;
pub mod leftovers;
pub mod nix_entry;
pub mod outgoing;
mod peer_invocations;
pub mod policy;
pub mod push_jobs;
pub mod rule_owners;
pub mod safe_push_verbs;
pub mod slug;
pub mod shell_hook;
pub mod strategy_purity;
pub mod strategy_replay;
mod stranded_boxes;
pub mod stale_revert;
pub mod test_admission;
mod test_file;
pub mod test_integrity;
pub mod tree_build;
pub mod tool_removal;
mod workflow_paths;
mod turn_scope;
mod wall_clock;
pub mod withdrawal;

use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: north land scan dep-gate|tool-removal|test-integrity|incident-test|test-admission|policy-sweep|generated-rewrite BASE..HEAD\n       north land scan push-jobs BASE..HEAD\n       north land scan test-admission run BASE..HEAD\n       north land scan test-admission kills [--host NAME] [--remote-north COMMAND] [--here] BASE..HEAD\n       north land scan private-file --range BASE..HEAD\n       north land scan boxes BASE..HEAD\n       north land scan docs-owners BASE..HEAD
       north land scan doc-links BASE..HEAD
       north land scan gate-shadow BASE..HEAD
       north land scan generated-registry BASE..HEAD
       north land scan holdlist-guards-resolve BASE..HEAD
       north land scan turn-scope BASE..HEAD
       north land scan detector-proof BASE..HEAD
       north land scan ci-budget|code-health|instruction-limits BASE..HEAD\n       north land scan denylist BASE HEAD REF\n       north land scan commit-email REVISION...\n       north land scan native-ref [--range REVISION]... [-- NAME...]\n       north land scan secrets RANGE\n       north land scan publication BASE HEAD\n       north land scan leftovers BASE..HEAD\n       north land scan shell-hook BASE..HEAD\n       north land scan strategy-replay BASE..HEAD\n       north land scan language --base BASE --head HEAD [--origin URL]\n       north land scan dropped-commits --lane TIP --head HEAD --onto BRANCH [--destination REVISION]\n       north land scan stale-revert [--repo DIR] --destination BRANCH --base OID [--lane TIP] HEAD\n       north land scan nix-entry [REVISION]\n       north land scan safe-push-verbs [REVISION]\n       north land scan run-time-data BASE..HEAD\n       north land scan deps\n       north land scan dep-floor LOCK < LOCK-BEFORE";

/// A bare revision or an empty base is a root commit: its base is the empty tree, so the diff and the history read are of the whole commit.
pub struct Range<'a> {
    pub base: &'a str,
    pub head: &'a str,
}

impl<'a> Range<'a> {
    pub fn parse(range: &'a str) -> Option<Self> {
        let Some((base, _)) = range.split_once("..") else {
            return (!range.is_empty()).then_some(Self { base: EMPTY_TREE, head: range });
        };
        let (_, head) = range.rsplit_once("..")?;
        match base.is_empty() {
            true => (!head.is_empty()).then_some(Self { base: EMPTY_TREE, head }),
            false => Some(Self { base, head }),
        }
    }
}

fn dep_gate(directory: &Path, range: &str) -> u8 {
    let Some(range) = Range::parse(range) else {
        eprintln!("{USAGE}");
        return 1;
    };
    match git::text(directory, &["rev-parse", "--show-toplevel"]) {
        Some(root) => crate::dep_gate::run_at(
            Path::new(root.trim_end_matches('\n')),
            range.base,
            range.head,
        ),
        None => {
            eprintln!("dep-gate: not inside a git repository");
            1
        }
    }
}

fn unrecognized(arguments: &[String]) -> u8 {
    outgoing::run(arguments).unwrap_or_else(|| {
        eprintln!("{USAGE}");
        64
    })
}

fn ranged(directory: &Path, name: &str, range: &str) -> Option<u8> {
    let mut out = std::io::stdout();
    Some(match name {
        "run-time-data" => north_base::run_time_data::run(directory, range),
        "dep-gate" => dep_gate(directory, range),
        "doc-links" => doc_links::scan(directory, range, &mut out),
        "docs-owners" => docs_owners::scan(directory, range, &north_github_mirror::store::Mirror::local(), &mut out),
        "detector-proof" => detector_proof::run(directory, range),
        "generated-registry" => generated_registry::scan(directory, range, &mut out),
        "gate-shadow" => gate_shadow::scan(directory, range, &north_base::platform::records_mirror::state_home(), &mut out),
        "holdlist-guards-resolve" => holdlist::scan(directory, range, &north_base::platform::records_mirror::state_home(), &mut out),
        "turn-scope" => turn_scope::scan(directory, range, &north_base::platform::records_mirror::state_home(), &mut out),
        "tool-removal" => {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            tool_removal::scan(directory, &home, range, &mut out)
        }
        _ => return ranged_checks(directory, name, range),
    })
}

fn ranged_checks(directory: &Path, name: &str, range: &str) -> Option<u8> {
    let mut out = std::io::stdout();
    Some(match name {
        "test-integrity" => match test_integrity::scan(directory, range, &mut out) {
            0 => incident_guard::scan(directory, range, &mut out),
            refused => refused,
        },
        "incident-test" => incident_guard::scan(directory, range, &mut out),
        "test-admission" => test_admission::scan(directory, range, &north_base::platform::records_mirror::state_home(), &mut out),
        "policy-sweep" => policy::sweep(directory, range, &mut out),
        "leftovers" => leftovers::scan(directory, range, &mut out),
        "strategy-replay" => strategy_replay::scan(directory, range, &mut out),
        "shell-hook" => shell_hook::scan(directory, range, &mut out),
        "generated-rewrite" => generated::scan(directory, range, &mut std::io::stderr()),
        _ => return None,
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let directory = PathBuf::from(".");
    match arguments {
        [name, lock] if name == "dep-floor" => crate::dep_gate::floor_run(Path::new(lock)),
        [name, rest @ ..] if name == "language" => language::run(rest),
        [name, rest @ ..] if name == "dropped-commits" => dropped_commits::run(&directory, rest),
        [name, rest @ ..] if name == "stale-revert" => stale_revert::run(rest),
        [name, rest @ ..] if name == "nix-entry" => nix_entry::run(&directory, rest),
        [name, rest @ ..] if name == "safe-push-verbs" => safe_push_verbs::run(&directory, rest),
        [name, kills, rest @ ..] if name == "test-admission" && kills == "kills" => test_admission::kills(&directory, rest, &mut std::io::stdout()),
        [name, run, range] if name == "test-admission" && run == "run" => test_admission::fail_to_pass(&directory, range, &mut std::io::stdout()),
        [name, range] => ranged(&directory, name, range).unwrap_or_else(|| unrecognized(arguments)),
        [] => {
            println!("{USAGE}");
            0
        }
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            0
        }
        _ => unrecognized(arguments),
    }
}
