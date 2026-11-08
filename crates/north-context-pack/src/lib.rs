//! `north work brief pack [--issue REF]... PATH...`: what a worker would otherwise spend its first tool
//! calls finding out about the files a box names, as one short block per file: its public items, its
//! structure.toml module, the files that changed with it, the tests that reach it, and what the fact
//! graph holds on it.
mod cochange;
mod outline;
mod pack;
mod placement;
mod tests_reaching;

use std::path::PathBuf;

const MOST_CHARACTERS: usize = 3_000;

pub fn run(arguments: &[String]) -> u8 {
    let mut issues = Vec::new();
    let mut paths = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match (word.as_str(), word.starts_with('-')) {
            ("--issue", _) => match words.next() {
                Some(issue) => issues.push(issue.clone()),
                None => return usage(),
            },
            (_, true) => return usage(),
            _ => paths.push(word.clone()),
        }
    }
    if paths.is_empty() {
        return usage();
    }
    let root = pack::root().unwrap_or_else(|| PathBuf::from("."));
    print!("{}", pack::render(&root, &paths, &issues, MOST_CHARACTERS));
    0
}

fn usage() -> u8 {
    eprintln!("usage: north work brief pack [--issue REF]... PATH...");
    2
}
