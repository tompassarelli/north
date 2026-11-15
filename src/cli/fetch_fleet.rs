use std::process::Command;

const VIEW: &str = "list";

pub(super) fn run(arguments: &[String]) -> u8 {
    let north = match std::env::current_exe() {
        Ok(north) => north,
        Err(error) => {
            eprintln!("fetch-fleet: {error}");
            return 1;
        }
    };
    let mut fleet = Command::new(north);
    fleet.arg("fleet");
    if arguments.is_empty() {
        fleet.arg(VIEW);
    } else {
        fleet.args(arguments);
    }
    for mut step in [fetch(), fleet, fetch()] {
        let code = match step.status() {
            Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
            Err(error) => {
                eprintln!("fetch-fleet: {}: {error}", step.get_program().to_string_lossy());
                1
            }
        };
        if code != 0 {
            return code;
        }
    }
    0
}

fn fetch() -> Command {
    let mut git = Command::new("git");
    git.arg("fetch");
    git
}
