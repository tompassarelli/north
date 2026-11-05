pub mod boxes;
pub mod dropped_commits;
pub mod generated;
pub(crate) mod git;
pub(crate) mod host_paths;
pub mod language;
pub mod nix_entry;
pub mod outgoing;
pub mod policy;
pub mod shell_hook;
pub mod stale_revert;
pub mod test_integrity;
pub mod tool_removal;

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: north land scan dep-gate|tool-removal|test-integrity|policy-sweep|generated-rewrite BASE..HEAD\n       north land scan boxes BASE..HEAD
       north land scan ci-budget|code-health|instruction-limits BASE..HEAD [--safe-push PATH]\n       north land scan denylist BASE HEAD REF\n       north land scan commit-email REVISION...\n       north land scan native-ref [--range REVISION]... [-- NAME...]\n       north land scan secrets RANGE\n       north land scan publication BASE HEAD\n       north land scan shell-hook BASE..HEAD\n       north land scan language --base BASE --head HEAD [--origin URL] [--safe-push PATH]\n       north land scan dropped-commits --lane TIP --head HEAD --onto BRANCH [--destination REVISION]\n       north land scan stale-revert [--repo DIR] --destination BRANCH --base OID [--lane TIP] HEAD\n       north land scan nix-entry [REVISION]\n       north land scan deps";

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
        [name, range] if name == "dep-gate" => dep_gate(&directory, range),
        [name, range] if name == "boxes" => boxes::run(&directory, range),
        [name, range] if name == "tool-removal" => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            tool_removal::scan(&directory, &home, range, &mut std::io::stdout())
        }
        [name, range] if name == "test-integrity" => {
            test_integrity::scan(&directory, range, &mut std::io::stdout())
        }
        [name, range] if name == "policy-sweep" => {
            policy::scan(&directory, range, &mut std::io::stdout())
        }
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
