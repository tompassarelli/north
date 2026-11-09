//! `north --worker-sandbox COMMAND`: runs one worker shell command under a Landlock domain in which a file or directory can be removed or renamed away only beneath the worker's lane, its git state and the scratch and cache trees its builds need. A delete whose target is computed at run time (an unset variable, a split `xargs` word, a `dirname` of the wrong line, a `remove_dir_all` on a computed path) meets the kernel with the real path, so the home directory, project containers, main checkouts, other lanes, pins and transcripts survive it. Creating and writing files stays open everywhere: the domain guards removal, not edits. Reading is open too, apart from the operator's own logins, since an agent calls GitHub as an App installation. See docs/rationale/worker-sandbox.md.
mod except;
mod readable;
mod removable;

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use landlock::{AccessFs, BitFlags, CompatLevel, Compatible, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};

pub use readable::readable;
pub use removable::{Context, removable};

pub const FLAG: &str = "--worker-sandbox";
const OUTSIDE: &str = "--outside";
const MARK: &str = "NORTH_WORKER_SANDBOXED";
const USAGE: &str = "usage: north --worker-sandbox COMMAND | north --worker-sandbox --outside -- PROGRAM [ARGS...]";

fn removal() -> BitFlags<AccessFs> {
    AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer
}

/// Restricts this process, and everything it starts, to removing and renaming away only beneath `roots` and to reading only files beneath `readable`.
pub fn confine(roots: &[PathBuf], readable: &[PathBuf]) -> Result<RulesetStatus, String> {
    let ruleset = landlock::Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(removal() | AccessFs::ReadFile).map_err(|error| error.to_string())?;
    let ruleset = ruleset.create().map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(roots.iter().filter(|root| root.is_dir()), removal())).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(readable, AccessFs::ReadFile)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.set_compatibility(CompatLevel::BestEffort).log_new_exec(true).map_err(|error| error.to_string())?;
    ruleset.restrict_self().map(|status| status.ruleset).map_err(|error| error.to_string())
}

pub fn run(arguments: &[String]) -> std::process::ExitCode {
    match arguments {
        [outside, separator, program @ ..] if outside == OUTSIDE && separator == "--" && !program.is_empty() => self::outside(program),
        [command] => shell(command),
        _ => {
            eprintln!("{USAGE}");
            std::process::ExitCode::from(2)
        }
    }
}

/// Runs `program` outside the worker's Landlock domain, as a transient user service with this environment, for a tool that must build its own mount namespace (Landlock refuses mount(2) inside a domain, so bubblewrap cannot start there); run unsandboxed, it just execs `program`. The tool's own namespace then confines what it may write.
fn outside(program: &[String]) -> std::process::ExitCode {
    if std::env::var_os(MARK).is_none() {
        let error = Command::new(&program[0]).args(&program[1..]).exec();
        eprintln!("north worker-sandbox: {}: {error}", program[0]);
        return std::process::ExitCode::from(126);
    }
    let mut service = Command::new("systemd-run");
    service.args(["--user", "--wait", "--pipe", "--quiet", "--collect", "--same-dir", "--service-type=exec"]);
    for (name, value) in std::env::vars_os().filter(|(name, _)| name != MARK) {
        let mut setting = OsString::from("--setenv=");
        setting.push(&name);
        setting.push("=");
        setting.push(&value);
        service.arg(setting);
    }
    let error = service.arg("--").args(program).exec();
    eprintln!("north worker-sandbox: systemd-run: {error}");
    std::process::ExitCode::from(126)
}

fn shell(command: &str) -> std::process::ExitCode {
    let context = Context::from_env(command);
    match confine(&removable(&context), &readable(&context)) {
        Ok(RulesetStatus::FullyEnforced) => {}
        Ok(status) => eprintln!("north worker-sandbox: Landlock {status:?} on this kernel, so removal and personal logins are not confined"),
        Err(error) => eprintln!("north worker-sandbox: Landlock unavailable ({error}), so removal and personal logins are not confined"),
    }
    let shell = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()).unwrap_or_else(|| OsString::from("bash"));
    let error = Command::new(shell).arg("-c").arg(command).env(MARK, "1").exec();
    eprintln!("north worker-sandbox: {error}");
    std::process::ExitCode::from(126)
}

/// Runs `north ARGUMENTS` outside the worker's Landlock domain when called inside it, for a verb that enforces its own removal rule (`north lane unlock`, `north lane stray`); `None` outside a worker sandbox, where the verb runs in place.
pub fn reenter_outside(arguments: &[String]) -> Option<std::process::ExitCode> {
    std::env::var_os(MARK)?;
    let north = std::env::current_exe().ok()?;
    let program: Vec<String> = std::iter::once(north.to_string_lossy().into_owned()).chain(arguments.iter().cloned()).collect();
    Some(outside(&program))
}

#[cfg(test)]
mod tests;
