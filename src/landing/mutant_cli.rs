use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::mutant::{Env, Lease, Stop, git_bytes, repo_name, valid_id};

const USAGE: &str = "Usage: north mutant record <id> | north mutant verify <base> <head>
Record the scratch copy diff against HEAD; verify each Test-kind kill claim before landing.";

fn process_env() -> Env {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let base = |name: &str, fallback: &str| match std::env::var_os(name) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home.join(fallback),
    };
    Env { state_home: base("XDG_STATE_HOME", ".local/state"), cache_home: base("XDG_CACHE_HOME", ".cache"), lease: lease(&home) }
}

fn lease(home: &Path) -> Lease {
    let cgroup = fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    if cgroup.split('/').any(|part| part.starts_with("agent-capacity-") && part.trim_end().ends_with(".scope")) {
        return Lease::Inherited;
    }
    let script = Path::new("skills/machine-capacity/scripts/machine-capacity.mjs");
    let candidates = vec![home.join(".claude").join(script), home.join("code").join("north").join("main").join("harness").join(script)];
    match candidates.iter().find(|candidate| candidate.is_file()) {
        Some(helper) => Lease::Helper(helper.clone()),
        None => Lease::Missing(candidates),
    }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let env = process_env();
    let outcome = std::env::current_dir().map_err(Stop::from).and_then(|cwd| match arguments {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        [command, id] if command == "record" => record(&cwd, id, &env),
        [command, base, head] if command == "verify" => super::mutant_verify::verify(&cwd, base, head, &env, &mut io::stdout()),
        _ => Err(Stop::Usage(USAGE.to_owned())),
    });
    match outcome {
        Ok(()) => 0,
        Err(Stop::Usage(message)) => {
            eprintln!("{message}");
            2
        }
        Err(Stop::Fail(message)) => {
            eprintln!("{message}");
            1
        }
        Err(Stop::Refused) => 1,
    }
}

pub(super) fn record(cwd: &Path, id: &str, env: &Env) -> Result<(), Stop> {
    if !valid_id(id) {
        return Err(Stop::Usage("mutant: invalid id".to_owned()));
    }
    let repo = repo_name(cwd)?;
    let diff = git_bytes(cwd, None, &["diff", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD"])?;
    if diff.is_empty() {
        return Err(Stop::Fail(format!("mutant: {id} has an empty diff; NOT recorded.")));
    }
    let patch = env.patch(&repo, id);
    fs::create_dir_all(env.mutants(&repo))?;
    let partial = PathBuf::from(format!("{}.tmp.{}", patch.display(), std::process::id()));
    fs::write(&partial, &diff)?;
    fs::rename(&partial, &patch)?;
    println!("mutant: recorded {}", patch.display());
    Ok(())
}

#[cfg(test)]
#[path = "mutant_tests.rs"]
mod tests;
