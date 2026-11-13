use std::path::PathBuf;

use north_base::platform::records_mirror::state_home;
use north_base::verdict_cache::reuse_log;

mod execute;
mod observe;
pub use observe::checked_environment;
pub mod outcome;
mod recorder;
pub(super) mod runs;

pub const USAGE: &str = "north land reuse observe --probe ID [--record] [--tree REV] -- COMMAND (runs the nextest land line under bash, passes its output and exit status through, then prints for each test the lookup of its recorded path sets as north-reuse lines; with --record, each test also runs under the sandbox recorder and its path set and verdict are stored; a `buck2 test` line is read back from Buck2's own record of the run instead, one north-reuse line per test binary, reused when its action's result came from the action cache) | available";
const RUNNER_KEY: &str = "target.x86_64-unknown-linux-gnu.runner";
pub const RECORD_ENV: &str = "NORTH_LAND_REUSE_RECORD";
const NO_RECORDING: [&str; 1] = ["north-land-sandbox"];

pub fn directory() -> PathBuf {
    reuse_log::directory(&state_home())
}

pub fn runs_nextest(command: &str) -> bool {
    command.contains("cargo nextest run") && !command.contains("--no-run")
}

/// Whether `command` runs tests the observer reads reuse for: a nextest run, or a run the build system records itself.
pub fn runs_tests(command: &str) -> bool {
    runs_nextest(command) || north_build_system::test_record(command).is_some()
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
