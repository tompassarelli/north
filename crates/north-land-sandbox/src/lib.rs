mod child;
mod classify;
mod fence;
mod not_inputs;
mod options;
mod paths;
mod recorder;
mod supervisor;

pub const USAGE: &str = "north land sandbox exec [--enforce PATHSET] [--record FILE|DIR] [--except BINARY_ID]... [--tree DIR] -- CMD... (a --record DIR writes one file per nextest unit, named by fingerprint::record_name of NEXTEST_BINARY_ID and NEXTEST_TEST_NAME, and runs a command without them, or of an --except binary, untraced)";

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
