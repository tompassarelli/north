mod git;
pub mod test_integrity;
pub mod tool_removal;

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: north land scan dep-gate|tool-removal|test-integrity BASE..HEAD";

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
        [name, range] if name == "dep-gate" => dep_gate(&directory, range),
        [name, range] if name == "tool-removal" => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            tool_removal::scan(&directory, &home, range, &mut std::io::stdout())
        }
        [name, range] if name == "test-integrity" => {
            test_integrity::scan(&directory, range, &mut std::io::stdout())
        }
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            64
        }
    }
}

#[cfg(test)]
#[path = "range_tests.rs"]
mod range_tests;
