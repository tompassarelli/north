use std::fs::{self, OpenOptions};
use std::process::Command;

use rustix::fs::{FlockOperation, flock};

use super::{date, env_or, home, north_program};

pub(super) fn steps(version: &str, exec: &mut dyn FnMut(&str, &[&str]) -> i32) -> Vec<String> {
    let plan: Vec<(String, Vec<&str>)> = vec![
        ("codex-runtime-update".to_owned(), vec![version]),
        (north_program().to_string_lossy().into_owned(), vec!["runtime", "update"]),
        ("claude-runtime-update".to_owned(), vec![version]),
        ("agy-runtime-update".to_owned(), vec![version]),
    ];
    plan.iter()
        .filter_map(|(program, args)| {
            let code = exec(program, args);
            (code != 0).then(|| format!("{} (exit {code})", [program.as_str()].into_iter().chain(args.iter().copied()).collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

fn spawn(program: &str, args: &[&str]) -> i32 {
    Command::new(program).args(args).status().map_or(127, |status| status.code().unwrap_or(1))
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.len() > 1 {
        return 64;
    }
    let state = env_or("XDG_STATE_HOME", || home().join(".local/state")).join("machine-updates");
    match update(&state, arguments.first().map_or("latest", String::as_str)) {
        Ok(failed) if failed.is_empty() => 0,
        Ok(failed) => {
            failed.iter().for_each(|item| eprintln!("agent-runtime-update: failed: {item}"));
            1
        }
        Err(error) => {
            eprintln!("agent-runtime-update: {error}");
            1
        }
    }
}

fn update(state: &std::path::Path, version: &str) -> Result<Vec<String>, String> {
    fs::create_dir_all(state).map_err(|error| format!("{}: {error}", state.display()))?;
    let lock = OpenOptions::new().create(true).append(true).open(state.join("agent-runtime-update.lock")).map_err(|error| error.to_string())?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
    let failed = steps(version, &mut spawn);
    if failed.is_empty() {
        let staged = state.join(format!(".agent-runtime-update.success.{}", std::process::id()));
        fs::write(&staged, format!("{}\n", date("+%s"))).and_then(|()| fs::rename(&staged, state.join("agent-runtime-update.success"))).map_err(|error| error.to_string())?;
    }
    Ok(failed)
}
