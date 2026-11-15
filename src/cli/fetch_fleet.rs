use std::process::Command;

use super::steps;

const NAME: &str = "fetch-fleet";
const VIEW: &str = "list";

pub(super) fn run(arguments: &[String]) -> u8 {
    let north = match steps::north(NAME) {
        Ok(north) => north,
        Err(code) => return code,
    };
    let mut fleet = Command::new(north);
    fleet.arg("fleet");
    if arguments.is_empty() {
        fleet.arg(VIEW);
    } else {
        fleet.args(arguments);
    }
    steps::run_in_order(NAME, vec![fetch(), fleet, fetch()])
}

fn fetch() -> Command {
    let mut git = Command::new("git");
    git.arg("fetch");
    git
}
