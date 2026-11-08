//! `north work brief pack PATH...`: what a worker would otherwise spend its first tool calls finding
//! out about the files a box names, as one short block per file: its public items, its structure.toml
//! module, the files that changed with it, and the tests that reach it.
mod cochange;
mod outline;
mod pack;
mod placement;
mod tests_reaching;

use std::path::PathBuf;

const MOST_CHARACTERS: usize = 3_000;

pub fn run(arguments: &[String]) -> u8 {
    if arguments.is_empty() || arguments.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("usage: north work brief pack PATH...");
        return 2;
    }
    let root = pack::root().unwrap_or_else(|| PathBuf::from("."));
    print!("{}", pack::render(&root, arguments, MOST_CHARACTERS));
    0
}
