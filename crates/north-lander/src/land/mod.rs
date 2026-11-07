pub use north_landing::land::*;

pub mod divergence;
pub mod enqueue;
pub mod first_commit;
pub mod link_hold;
pub mod pause;
pub mod probes;
pub mod promote;
pub mod push_refusal;
pub mod red;
pub mod replay;
pub mod reuse;
pub mod serve;
pub mod shadow;
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
