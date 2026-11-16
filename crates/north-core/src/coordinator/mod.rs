//! The coordinator's tool calls by kind: the classifier the delegation guard and the daily readout share.
pub mod calls;
mod readout;
mod shape;
mod shell;
mod streak;

pub use calls::{Kind, VerbKinds, classify};
pub use readout::{Readout, read, render, render_read_streaks, streaks};
pub use shape::shape;
pub use streak::{Streak, read_streaks};

const USAGE: &str = "usage: north config coordinator calls TRANSCRIPT | north config coordinator role";

pub fn run(arguments: &[String], verbs: &VerbKinds, role: fn(&[String]) -> u8) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "role" => role(rest),
        Some((verb, [path])) if verb == "calls" => match std::fs::read_to_string(path) {
            Ok(text) => {
                println!("{}", render(&read(&text, verbs)));
                println!("{}", render_read_streaks(&read_streaks(&text, verbs)));
                0
            }
            Err(error) => {
                eprintln!("north config coordinator calls: cannot read {path}: {error}");
                1
            }
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
