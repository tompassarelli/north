mod execute;
pub mod harvest;
pub mod lines;
mod observe;
pub mod outcome;
mod recorder;

pub const USAGE: &str = "north land reuse observe --probe ID [--record] [--tree REV] -- COMMAND (runs the nextest land line under bash, passes its output and exit status through, then prints for each test the lookup of its recorded path sets as north-reuse lines; with --record, each test also runs under the sandbox recorder and its path set and verdict are stored) | available";
const RUNNER_KEY: &str = "target.x86_64-unknown-linux-gnu.runner";
pub const RECORD_ENV: &str = "NORTH_LAND_REUSE_RECORD";
const NO_RECORDING: [&str; 1] = ["north-land-sandbox"];

pub fn runs_nextest(command: &str) -> bool {
    command.contains("cargo nextest run") && !command.contains("--no-run")
}

pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

pub fn wrapped(north: &str, probe: &str, record: bool, command: &str) -> String {
    let flags = if record { "--record " } else { "" };
    format!("n={north}; if \"$n\" land reuse available >/dev/null 2>&1; then exec \"$n\" land reuse observe --probe {} {flags}-- {}; fi\n{command}", quote(probe), quote(command))
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, _)) if verb == "available" => 0,
        Some((verb, rest)) if verb == "observe" => observe::run(rest),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}

#[cfg(test)]
mod tests;
