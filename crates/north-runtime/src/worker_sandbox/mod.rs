//! `north --worker-sandbox COMMAND`: runs one shell command of an agent session under a Landlock domain. Every agent session's domain keeps the operator's own logins unreadable, since an agent calls GitHub as an App installation, and sets git's GitHub credential helper to that App. It also refuses to execute build output (a lane's `target/` and `node_modules/`) by its path, so the code a lane's dependencies bring runs only inside depbox, which the toolchain shims enter, or through `north lane exec` for a lane whose lock files equal main's. A worker's domain also confines removal: a file or directory can be removed or renamed away only beneath the worker's lane, its git state and the scratch and cache trees its builds need. A delete whose target is computed at run time (an unset variable, a split `xargs` word, a `dirname` of the wrong line, a `remove_dir_all` on a computed path) meets the kernel with the real path, so the home directory, project containers, main checkouts, other lanes, pins and transcripts survive it. Creating and writing files stays open everywhere: the domain guards removal, not edits. Reading is open apart from the logins. See docs/rationale/worker-sandbox.md.
mod except;
mod executable;
mod readable;
mod removable;

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use north_base::platform::git_credential;
use landlock::{AccessFs, BitFlags, CompatLevel, Compatible, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};

pub use executable::{build_outputs, executable};
pub use readable::readable;
pub use removable::{Context, removable};

pub const FLAG: &str = "--worker-sandbox";
const OUTSIDE: &str = "--outside";
const BUILT: &str = "--built";
const MARK: &str = north_base::platform::gh_session::SANDBOX_MARK;
const ROLE_ENV: &str = "NORTH_FLEET_ROLE";
const USAGE: &str = "usage: north --worker-sandbox COMMAND | north --worker-sandbox --outside|--built -- PROGRAM [ARGS...]";

fn removal() -> BitFlags<AccessFs> {
    AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer
}

/// Restricts this process, and everything it starts, to reading only files beneath `readable`, given `executable` to executing only files beneath it, and given `roots` to removing and renaming away only beneath them. Refer is handled either way, so a file cannot be linked or moved to where it would become readable or executable.
pub fn confine(roots: Option<&[PathBuf]>, readable: &[PathBuf], executable: Option<&[PathBuf]>) -> Result<RulesetStatus, String> {
    let handled = if roots.is_some() { removal() } else { BitFlags::from(AccessFs::Refer) };
    let handled = if executable.is_some() { handled | AccessFs::Execute } else { handled };
    let ruleset = landlock::Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(handled | AccessFs::ReadFile).map_err(|error| error.to_string())?;
    let ruleset = ruleset.create().map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(roots.unwrap_or_default().iter().filter(|root| root.is_dir()), removal())).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(readable, AccessFs::ReadFile)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(executable.unwrap_or_default(), AccessFs::Execute)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.set_compatibility(CompatLevel::BestEffort).log_new_exec(true).map_err(|error| error.to_string())?;
    ruleset.restrict_self().map(|status| status.ruleset).map_err(|error| error.to_string())
}

pub fn run(arguments: &[String]) -> std::process::ExitCode {
    match arguments {
        [outside, separator, program @ ..] if outside == OUTSIDE && separator == "--" && !program.is_empty() => self::outside(program),
        [built, separator, program @ ..] if built == BUILT && separator == "--" && !program.is_empty() => self::built(program),
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
    let settable = |name: &OsString| name.to_str().is_some_and(|name| name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    for (name, value) in std::env::vars_os().filter(|(name, _)| name != MARK && settable(name)) {
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

/// Removal is confined for a fleet worker once the `worker-sandbox` gate of harness/gates.toml enforces, or for a canary launch made with NORTH_WORKER_SANDBOX=1 while it is in shadow (north#706); the proxy, leads and sessions started by hand keep removal open.
fn confines_removal() -> bool {
    let worker = std::env::var_os(ROLE_ENV).is_some_and(|role| role == "worker");
    let applies = north_base::platform::harness::document("harness/gates.toml")["gate"]["worker-sandbox"]["mode"].as_str() == Some("enforce") || std::env::var_os("NORTH_WORKER_SANDBOX").is_some_and(|canary| canary == "1");
    worker && applies
}

fn shell(command: &str) -> std::process::ExitCode {
    let context = Context::from_env(command);
    let roots = confines_removal().then(|| removable(&context));
    report(confine(roots.as_deref(), &readable(&context), Some(&executable(&context))));
    let shell = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()).unwrap_or_else(|| OsString::from("bash"));
    let error = Command::new(shell).arg("-c").arg(command).envs(git_credential::session_env()).env(MARK, "1").exec();
    eprintln!("north worker-sandbox: {error}");
    std::process::ExitCode::from(126)
}

/// Runs `program`, a lane's own build output that `north lane exec` vouched for, under the command's domain without the rule against executing build output.
fn built(program: &[String]) -> std::process::ExitCode {
    let context = Context::from_env(&program.join(" "));
    let roots = confines_removal().then(|| removable(&context));
    report(confine(roots.as_deref(), &readable(&context), None));
    let error = Command::new(&program[0]).args(&program[1..]).envs(git_credential::session_env()).env(MARK, "1").exec();
    eprintln!("north worker-sandbox: {}: {error}", program[0]);
    std::process::ExitCode::from(126)
}

fn report(confined: Result<RulesetStatus, String>) {
    match confined {
        Ok(RulesetStatus::FullyEnforced) => {}
        Ok(status) => eprintln!("north worker-sandbox: Landlock {status:?} on this kernel, so removal, personal logins and build output are not confined"),
        Err(error) => eprintln!("north worker-sandbox: Landlock unavailable ({error}), so removal, personal logins and build output are not confined"),
    }
}

/// Runs `north ARGUMENTS` outside the worker's Landlock domain when called inside it, for a verb that enforces its own removal rule (`north lane unlock`, `north lane stray`); `None` outside a worker sandbox, where the verb runs in place.
pub fn reenter_outside(arguments: &[String]) -> Option<std::process::ExitCode> {
    std::env::var_os(MARK)?;
    let north = std::env::current_exe().ok()?;
    let program: Vec<String> = std::iter::once(north.to_string_lossy().into_owned()).chain(arguments.iter().cloned()).collect();
    Some(outside(&program))
}

/// Whether this process runs inside a worker sandbox's Landlock domain.
pub fn confined() -> bool {
    std::env::var_os(MARK).is_some()
}

/// Runs `program` without the domain's rule against executing build output, keeping its other rules, when called inside a worker sandbox; `None` outside one, where nothing refuses it.
pub fn run_built(program: &[String]) -> Option<std::process::ExitCode> {
    std::env::var_os(MARK)?;
    let north = std::env::current_exe().ok()?;
    let words: Vec<String> = [north.to_string_lossy().into_owned(), FLAG.to_owned(), BUILT.to_owned(), "--".to_owned()].into_iter().chain(program.iter().cloned()).collect();
    Some(outside(&words))
}

#[cfg(test)]
mod tests;
