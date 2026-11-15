use std::path::Path;
use std::process::Command;

use super::steps;

const NAME: &str = "fleet-issue";
const VIEW: &str = "list";

pub(super) fn run(arguments: &[String]) -> u8 {
    let north = match steps::north(NAME) {
        Ok(north) => north,
        Err(code) => return code,
    };
    let issue = if arguments.is_empty() { vec![VIEW.to_owned()] } else { arguments.to_vec() };
    steps::run_in_order(NAME, vec![fleet(&north), gh_issue(issue), fleet(&north)])
}

fn fleet(north: &Path) -> Command {
    let mut command = Command::new(north);
    command.arg("fleet").arg(VIEW);
    command
}

fn gh_issue(arguments: Vec<String>) -> Command {
    let mut gh = Command::new("gh");
    gh.arg("issue").args(arguments);
    gh
}
