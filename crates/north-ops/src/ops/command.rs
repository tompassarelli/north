use crate::{check, dividend};

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "dividend" => dividend::run(rest),
        Some((command, _)) if command == "check" => check::run(arguments),
        _ => {
            eprintln!("usage: north ops dividend [--dry-run] [CHECK] | north ops check [--dry-run]");
            2
        }
    }
}
