use std::path::Path;
use std::process::Command;

use super::steps;

const NAME: &str = "fleet-for";
const OVERVIEW: &str = "list";
const DETAIL: &str = "show";

pub(super) fn run(arguments: &[String]) -> u8 {
    if arguments.is_empty() {
        eprintln!("usage: north fleet-for NAME...");
        return 2;
    }
    let north = match steps::north(NAME) {
        Ok(north) => north,
        Err(code) => return code,
    };
    let mut sequence = vec![fleet(&north, &[OVERVIEW])];
    sequence.extend(arguments.iter().map(|session| fleet(&north, &[DETAIL, session.as_str()])));
    sequence.push(fleet(&north, &[OVERVIEW]));
    steps::run_in_order(NAME, sequence)
}

fn fleet(north: &Path, words: &[&str]) -> Command {
    let mut command = Command::new(north);
    command.arg("fleet").args(words);
    command
}
