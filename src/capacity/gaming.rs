use super::agent_load::{self, Host};

const USAGE: &str = "usage: north capacity gaming on | off | status";

pub(crate) fn run(arguments: &[String]) -> u8 {
    let host = Host::from_env();
    let declare = |gaming: bool| match agent_load::declare_gaming(&host.capacity, gaming) {
        Ok(()) => {
            println!("gaming {}", if gaming { "on" } else { "off" });
            0
        }
        Err(error) => {
            eprintln!("north capacity gaming: {}: {error}", host.capacity.display());
            1
        }
    };
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["on"] => declare(true),
        ["off"] => declare(false),
        ["status"] => {
            let gaming = agent_load::gaming_marked(&host.capacity);
            println!("gaming {}", if gaming { "on" } else { "off" });
            u8::from(!gaming)
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
