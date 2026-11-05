pub mod after;
pub mod bisect;
pub mod candidate;
pub mod check_coverage;
pub mod checks;
pub mod credential;
pub mod culprit;
pub mod enqueue;
pub mod first_commit;
pub mod incidents;
pub mod lander;
pub mod pause;
pub mod precheck;
pub mod promote;
pub mod queue;
pub mod push_refusal;
pub mod post_land;
pub mod records;
pub mod reuse;
pub mod red;
pub mod refusal;
pub mod replay;
pub mod scans;
pub mod serve;
pub mod sheriff;
pub mod slots;
pub mod speculate;
pub mod urgent;
pub mod withdraw;
pub mod telemetry;
pub mod watchdog;
pub mod repositories;
pub mod setting;

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE]\n       north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]\n       north land enqueue [--repo DIR] [--message FILE]\n       north land urgent TICKET --reason TEXT [--repo DIR]\n       north land withdraw TICKET --reason TEXT [--repo DIR] [--noticed-by WHO] [--class CLASS]\n       north land scan dep-gate|tool-removal|test-integrity BASE..HEAD\n       north land watchdog --once [--queue-root DIR]\n       north land promote [--dry-run] [--shadow-minutes N] [--repo DIR]... [--checker PROGRAM]\n       north land replay --base REV [--repo DIR] [--checker PROGRAM] TICKET...\n       north land reuse hash|record|lookup";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "serve" => serve::run(rest),
        Some((command, rest)) if command == "promote" => promote::run(rest),
        Some((command, rest)) if command == "replay" => replay::run(rest),
        Some((command, rest)) if command == "enqueue" => enqueue::run(rest),
        Some((command, rest)) if command == "follow" => after::follow::run(rest),
        Some((command, rest)) if command == "checks" => slots::runner::run(rest),
        Some((command, rest)) if command == "scan" => scans::run(rest),
        Some((command, rest)) if command == "urgent" => urgent::run(rest),
        Some((command, rest)) if command == "reuse" => reuse::run(rest),
        Some((command, rest)) if command == "withdraw" => withdraw::run(rest),
        Some((command, rest)) if command == "watchdog" => watchdog::run(rest),
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
