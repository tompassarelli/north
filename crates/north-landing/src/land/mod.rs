pub mod after;
pub mod bisect;
pub mod blame;
pub mod candidate;
mod steps;
pub mod judge_case;
pub mod check_coverage;
pub mod checks;
pub mod credential;
pub mod culprit;
pub mod enqueue;
pub mod first_commit;
pub mod heartbeat;
pub mod hold;
pub mod lane_gates;
pub mod incidents;
pub mod jobs;
pub mod lander;
pub mod model_cmd;
pub mod outcomes;
pub mod pause;
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
pub mod trace;
pub mod watch;
pub mod watchdog;
pub mod repositories;
pub mod reservation;
pub mod setting;
pub mod shadow;

pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub run: fn(&[String]) -> u8,
}

pub const COMMANDS: &[Command] = include!("commands.rs");

pub fn usage() -> String {
    let lines = COMMANDS.iter().filter(|command| !command.usage.is_empty()).map(|command| command.usage).collect::<Vec<_>>();
    format!("usage: {}", lines.join("\n       "))
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((name, rest)) if let Some(command) = COMMANDS.iter().find(|command| command.name == *name) => (command.run)(rest),
        Some((name, _)) if name == "-h" || name == "--help" => {
            println!("{}", usage());
            0
        }
        _ => {
            eprintln!("{}", usage());
            64
        }
    }
}
