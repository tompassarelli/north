mod allowlist;
mod guard;
mod violations;

use std::path::Path;
use std::process::ExitCode;
use violations::violations;

pub use guard::run_guard;

const ALLOWLIST: &str = ".language-allowlist.json";
/// A Python entry whose gap is exactly this is a library that only has a Python interface, so it carries no Rust port record.
const LIBRARY_GAP: &str = "library";
const ALTERNATIVE: &str = "write it in Rust under src/ or as a north subcommand";
const INPUT_LIMIT: u64 = 1024 * 1024;
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

type Result<T> = std::result::Result<T, String>;

pub fn run_gate(args: &[String]) -> ExitCode {
    let result = (|| {
        let mut repo = None;
        let mut base = None;
        let mut head = None;
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or("Usage: north language-gate --repo DIR --base REV --head REV")?;
            let slot = match flag.as_str() {
                "--repo" => &mut repo,
                "--base" => &mut base,
                "--head" => &mut head,
                _ => return Err(format!("unknown language-gate option {flag}")),
            };
            if slot.replace(value.as_str()).is_some() {
                return Err(format!("duplicate option {flag}"));
            }
        }
        let (Some(repo), Some(base), Some(head)) = (repo, base, head) else {
            return Err("Usage: north language-gate --repo DIR --base REV --head REV".into());
        };
        violations(Path::new(repo), base, head)
    })();
    if report(result) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

pub fn check(repo: &Path, base: &str, head: &str) -> bool {
    report(violations(repo, base, head))
}

fn report(result: Result<Vec<String>>) -> bool {
    match result {
        Ok(failures) if failures.is_empty() => {
            println!("language-gate: passed");
            true
        }
        Ok(failures) => {
            for failure in failures {
                println!("{failure}");
            }
            false
        }
        Err(error) => {
            eprintln!("language-gate: {error}");
            false
        }
    }
}

#[cfg(test)]
mod tests;
