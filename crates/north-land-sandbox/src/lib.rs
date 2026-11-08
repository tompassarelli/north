mod child;
mod classify;
mod fence;
mod options;
mod recorder;
mod supervisor;

pub const USAGE: &str = "north land sandbox exec [--enforce PATHSET] [--record FILE] [--tree DIR] -- CMD...";

const CHILD_VERB: [&str; 3] = ["land", "sandbox", "child"];
const SANDBOX_FAILED: u8 = 125;

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "exec" => supervisor::run(rest),
        Some((verb, rest)) if verb == "child" => child::run(rest),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}
