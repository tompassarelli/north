use std::path::Path;
use std::process::ExitCode;
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::git as platform_git;

type Result<T> = std::result::Result<T, String>;

const TRAILER: &str = "Build-cache-off";
const USAGE: &str = "Usage: north build-cache-gate --repo DIR --base REV --head REV";
const REMEDY: &str = "keep the rustc wrapper and build incrementally with CARGO_BUILD_INCREMENTAL=true (sccache passes an incremental crate through and caches the rest), or name the reason in a Build-cache-off: trailer";

static WRAPPER_OFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"RUSTC_WRAPPER(?:=(?:""|''|$|[\s;,&|)}])|\s*[:=]\s*(?:""|''|$|[;,&|)}]))|"RUSTC_WRAPPER"\s*,\s*""|(?:-u|--unset)[ =]RUSTC_WRAPPER\b|\bunset\s+(?:-v\s+)?RUSTC_WRAPPER\b|env_remove\(\s*"RUSTC_WRAPPER"|\bdelete\s+\S*RUSTC_WRAPPER|rustc-wrapper\s*=\s*(?:""|'')"#).expect("wrapper pattern")
});

static INCREMENTAL_ENV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"CARGO_INCREMENTAL\s*[:=]\s*["']?1\b|"CARGO_INCREMENTAL"\s*,\s*"1""#).expect("incremental pattern"));

/// What an added line does to the compiler cache: turn the rustc wrapper off, or set CARGO_INCREMENTAL, under which sccache 0.15 refuses every compile.
pub fn refusal(line: &str) -> Option<&'static str> {
    if WRAPPER_OFF.is_match(line) {
        Some("turns the rustc wrapper (sccache) off, so every crate recompiles with no cache")
    } else if INCREMENTAL_ENV.is_match(line) {
        Some("sets CARGO_INCREMENTAL, under which sccache refuses every compile")
    } else {
        None
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    platform_git::stdout(repo, args, |output| format!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim()))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn reasons(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let log = git(repo, &["log", &format!("--format=%(trailers:key={TRAILER},valueonly)"), &format!("{base}..{head}")])?;
    Ok(log.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

/// Each added line between `base` and `head` that turns the compiler cache off, as `PATH: what it does: line`.
pub fn violations(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let diff = git(repo, &["diff", "--no-color", "--no-ext-diff", "--unified=0", base, head])?;
    let mut path = String::new();
    let mut found = Vec::new();
    for line in diff.lines() {
        if let Some(name) = line.strip_prefix("+++ ") {
            name.strip_prefix("b/").unwrap_or(name).clone_into(&mut path);
        } else if let Some(added) = line.strip_prefix('+')
            && let Some(problem) = refusal(added)
        {
            found.push(format!("{path}: {problem}: {}", added.trim()));
        }
    }
    Ok(found)
}

fn report(repo: &Path, base: &str, head: &str) -> Result<bool> {
    let found = violations(repo, base, head)?;
    if found.is_empty() {
        println!("build-cache-gate: passed");
        return Ok(true);
    }
    let reasons = reasons(repo, base, head)?;
    for line in &found {
        println!("build-cache-gate: {line}");
    }
    if let Some(reason) = reasons.first() {
        println!("build-cache-gate: passed on the recorded reason: {reason}");
        return Ok(true);
    }
    println!("build-cache-gate: {REMEDY}");
    Ok(false)
}

pub fn check(repo: &Path, base: &str, head: &str) -> bool {
    report(repo, base, head).unwrap_or_else(|error| {
        eprintln!("build-cache-gate: {error}");
        false
    })
}

pub fn run_gate(args: &[String]) -> ExitCode {
    let parsed = (|| {
        let (mut repo, mut base, mut head) = (None, None, None);
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args.next().ok_or(USAGE)?;
            let slot = match flag.as_str() {
                "--repo" => &mut repo,
                "--base" => &mut base,
                "--head" => &mut head,
                _ => return Err(format!("unknown build-cache-gate option {flag}")),
            };
            if slot.replace(value.clone()).is_some() {
                return Err(format!("duplicate option {flag}"));
            }
        }
        repo.zip(base).zip(head).map(|((repo, base), head)| (repo, base, head)).ok_or_else(|| USAGE.to_owned())
    })();
    match parsed {
        Ok((repo, base, head)) if check(Path::new(&repo), &base, &head) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("build-cache-gate: {error}");
            ExitCode::from(2)
        }
    }
}
