//! Runs every case of one libtest binary in its own process, as nextest does, and prints one nextest-style status line per case, so a Buck2 test target reports the verdicts and the count `cargo nextest run` reports.
mod cases;
mod report;
mod run;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage: north-test-cases --binary-id ID [--threads N] -- BINARY";

struct Options {
    binary_id: String,
    threads: usize,
    binary: PathBuf,
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|options| test(&options)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(report::FAILED),
        Err(error) => {
            eprintln!("north-test-cases: {error}");
            ExitCode::from(report::BROKEN)
        }
    }
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let (mut binary_id, mut threads) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--binary-id" => binary_id = args.next(),
            "--threads" => threads = Some(args.next().and_then(|count| count.parse().ok()).filter(|&count| count > 0).ok_or(USAGE)?),
            "--" => break,
            _ => return Err(USAGE.to_owned()),
        }
    }
    let binary = args.next().map(PathBuf::from).ok_or(USAGE)?;
    let threads = threads.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
    Ok(Options { binary_id: binary_id.ok_or(USAGE)?, threads, binary })
}

fn test(options: &Options) -> Result<bool, String> {
    let started = Instant::now();
    let listed = cases::list(&options.binary)?;
    let outcomes = run::all(&options.binary, &listed.run, options.threads, &run::Home::new()?)?;
    Ok(report::print(&options.binary_id, &outcomes, listed.ignored, started.elapsed()))
}
