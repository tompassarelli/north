//! The proxy's tool calls by kind: the classifier the delegation guard and the daily readout share.
pub mod calls;
mod readout;
mod shape;
mod shell;

pub use calls::{Kind, VerbKinds, classify};
pub use readout::{Readout, read, render, streaks};
pub use shape::shape;

const USAGE: &str = "usage: north proxy calls TRANSCRIPT";

pub fn run(arguments: &[String], verbs: &VerbKinds) -> u8 {
    let [verb, path] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if verb != "calls" {
        eprintln!("{USAGE}");
        return 2;
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            println!("{}", render(&read(&text, verbs)));
            0
        }
        Err(error) => {
            eprintln!("north proxy calls: cannot read {path}: {error}");
            1
        }
    }
}
