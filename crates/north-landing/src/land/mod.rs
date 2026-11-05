pub mod after;
pub mod bisect;
pub mod candidate;
pub mod checks;
pub mod clippy;
pub mod credential;
pub mod culprit;
pub mod enqueue;
pub mod first_commit;
pub mod lander;
pub mod pause;
pub mod precheck;
pub mod queue;
pub mod push_refusal;
pub mod post_land;
pub mod records;
pub mod red;
pub mod refusal;
pub mod scans;
pub mod serve;
pub mod sheriff;
pub mod slots;
pub mod speculate;
pub mod telemetry;
pub mod repositories;
pub mod setting;

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE]\n       north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]\n       north land enqueue [--repo DIR] [--message FILE]\n       north land scan dep-gate|tool-removal|test-integrity BASE..HEAD";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "serve" => serve::run(rest),
        Some((command, rest)) if command == "enqueue" => enqueue::run(rest),
        Some((command, rest)) if command == "follow" => after::follow::run(rest),
        Some((command, rest)) if command == "checks" => slots::runner::run(rest),
        Some((command, rest)) if command == "scan" => scans::run(rest),
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
