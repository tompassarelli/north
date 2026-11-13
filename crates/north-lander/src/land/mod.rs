pub use north_landing::land::*;

pub mod break_glass;
pub mod build_lag;
pub mod committed_lane;
pub mod divergence;
pub mod enqueue;
pub mod first_commit;
pub mod hand_in;
pub mod lander_fix;
pub mod link_hold;
pub mod map_lag;
pub mod pause;
pub mod promote;
pub mod push_refusal;
pub mod replay;
pub mod serve;
pub mod shadow;
pub mod status;
pub mod unit_start;
pub mod urgent;
pub mod watch;
pub mod watchdog;
pub mod withdraw;

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
    if let Err(problem) = north_landing::land::queue::confined(&north_landing::land::lander::state_home()) {
        eprintln!("north land: {problem}");
        return 1;
    }
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

#[cfg(test)]
mod landed_twice_tests;
