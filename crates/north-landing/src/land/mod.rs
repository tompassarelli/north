pub mod after;
pub mod bisect;
pub mod candidate;
pub mod checks;
pub mod enqueue;
pub mod first_commit;
pub mod lander;
pub mod queue;
pub mod records;
pub mod red;
pub mod serve;
pub mod slots;
pub mod speculate;
pub mod telemetry;

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE]\n       north land serve [--once] [--repo DIR] [--checker PROGRAM] [--depth 1|2]\n       north land enqueue [--repo DIR] [--message FILE]";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "serve" => serve::run(rest),
        Some((command, rest)) if command == "enqueue" => enqueue::run(rest),
        Some((command, rest)) if command == "follow" => after::follow::run(rest),
        Some((command, rest)) if command == "checks" => slots::runner::run(rest),
        Some((command, _)) if command == "-h" || command == "--help" => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            64
        }
    }
}
