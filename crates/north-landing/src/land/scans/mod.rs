pub mod boxes;
pub mod doc_links;
mod doc_paths;
pub mod docs_owners;
pub mod dropped_commits;
pub mod generated;
pub(crate) mod git;
pub(crate) mod host_paths;
pub mod incident_guard;
pub mod language;
pub mod nix_entry;
pub mod outgoing;
pub mod policy;
pub mod shell_hook;
pub mod strategy_replay;
pub mod stale_revert;
pub mod test_admission;
pub mod test_integrity;
pub mod tool_removal;

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: north land scan dep-gate|tool-removal|test-integrity|incident-test|test-admission|policy-sweep|generated-rewrite BASE..HEAD\n       north land scan test-admission run BASE..HEAD\n       north land scan test-admission kills [--host NAME] [--remote-north COMMAND] [--here] BASE..HEAD\n       north land scan boxes BASE..HEAD\n       north land scan docs-owners BASE..HEAD
       north land scan doc-links BASE..HEAD
       north land scan ci-budget|code-health|instruction-limits BASE..HEAD\n       north land scan denylist BASE HEAD REF\n       north land scan commit-email REVISION...\n       north land scan native-ref [--range REVISION]... [-- NAME...]\n       north land scan secrets RANGE\n       north land scan publication BASE HEAD\n       north land scan shell-hook BASE..HEAD\n       north land scan strategy-replay BASE..HEAD\n       north land scan language --base BASE --head HEAD [--origin URL]\n       north land scan dropped-commits --lane TIP --head HEAD --onto BRANCH [--destination REVISION]\n       north land scan stale-revert [--repo DIR] --destination BRANCH --base OID [--lane TIP] HEAD\n       north land scan nix-entry [REVISION]\n       north land scan run-time-data BASE..HEAD\n       north land scan deps";

struct Range<'a> {
    base: &'a str,
    head: &'a str,
}

impl<'a> Range<'a> {
    fn parse(range: &'a str) -> Option<Self> {
        let (base, _) = range.split_once("..")?;
        let (_, head) = range.rsplit_once("..")?;
        (!base.is_empty()).then_some(Self { base, head })
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

pub fn run(arguments: &[String]) -> u8 {
    let directory = PathBuf::from(".");
    match arguments {
        [name, rest @ ..] if name == "language" => language::run(rest),
        [name, rest @ ..] if matches!(name.as_str(), "ci-budget" | "code-health" | "crate-lines" | "instruction-limits") => crate::land::after::gates::scan(name, rest),
        [name, rest @ ..] if name == "dropped-commits" => dropped_commits::run(&directory, rest),
        [name, rest @ ..] if name == "stale-revert" => stale_revert::run(rest),
        [name] if name == "deps" => super::slots::deps::run(&directory),
        [name, rest @ ..] if name == "nix-entry" => nix_entry::run(&directory, rest),
        [name, range] if name == "run-time-data" => north_base::run_time_data::run(&directory, range),
        [name, range] if name == "dep-gate" => dep_gate(&directory, range),
        [name, range] if name == "doc-links" => doc_links::scan(&directory, range, &mut std::io::stdout()),
        [name, range] if name == "docs-owners" => {
            docs_owners::scan(&directory, range, &north_github_mirror::store::Mirror::local(), &mut std::io::stdout())
        }
        [name, range] if name == "boxes" => boxes::run(&directory, range),
        [name, range] if name == "tool-removal" => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            tool_removal::scan(&directory, &home, range, &mut std::io::stdout())
        }
        [name, range] if name == "test-integrity" => {
            let mut out = std::io::stdout();
            match test_integrity::scan(&directory, range, &mut out) {
                0 => incident_guard::scan(&directory, range, &mut out),
                refused => refused,
            }
        }
        [name, range] if name == "incident-test" => {
            incident_guard::scan(&directory, range, &mut std::io::stdout())
        }
        [name, kills, rest @ ..] if name == "test-admission" && kills == "kills" => {
            test_admission::kills(&directory, rest, &mut std::io::stdout())
        }
        [name, run, range] if name == "test-admission" && run == "run" => {
            test_admission::fail_to_pass(&directory, range, &mut std::io::stdout())
        }
        [name, range] if name == "test-admission" => {
            test_admission::scan(&directory, range, &mut std::io::stdout())
        }
        [name, range] if name == "policy-sweep" => {
            policy::scan(&directory, range, &mut std::io::stdout())
        }
        [name, range] if name == "strategy-replay" => strategy_replay::scan(&directory, range, &mut std::io::stdout()),
        [name, range] if name == "shell-hook" => {
            shell_hook::scan(&directory, range, &mut std::io::stdout())
        }
        [name, range] if name == "generated-rewrite" => {
            generated::scan(&directory, range, &mut std::io::stderr())
        }
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            0
        }
        _ => outgoing::run(arguments).unwrap_or_else(|| {
            eprintln!("{USAGE}");
            64
        }),
    }
}
