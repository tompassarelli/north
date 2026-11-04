use super::agent_load::{self, Host};
use super::mode;

const USAGE: &str = "usage: north capacity gaming on | off | status";

fn declare(gaming: bool, status: u8) -> u8 {
    if status != 0 {
        return status;
    }
    let host = Host::from_env();
    match agent_load::declare_gaming(&host.capacity, gaming) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north capacity gaming: {}: {error}", host.capacity.display());
            1
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["on"] => declare(true, mode::run(&["hold".to_owned(), "north capacity gaming on".to_owned()])),
        ["off"] => declare(false, mode::run(&["release".to_owned()])),
        ["status"] => {
            let state = mode::read(&mode::state_dir());
            println!("{}", mode::status_line(&state, &mode::Limits::bundled()));
            u8::from(state.mode != mode::Mode::Play)
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
