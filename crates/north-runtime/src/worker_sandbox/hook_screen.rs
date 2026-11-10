//! The hook deciders a worker command meets inside `north --worker-sandbox` when its harness binds no hook (north#1041). OpenCode and Pi run no north hook, so their shell commands are judged here before the domain is built; Claude Code sets `CLAUDECODE` for every command it runs after its own hook judged it with the session's full context, so those pass unjudged. The deciders live in north-guard, which this crate does not depend on: the binary passes them in as a [`Screen`].
use std::path::Path;
use std::process::ExitCode;

const HOOKED: &str = "CLAUDECODE";
const SHELLS: [&str; 4] = ["sh", "bash", "dash", "zsh"];

/// The refusal north's Bash hook gives a shell command run in a directory, or `None` to let it run.
pub type Screen = fn(&str, &Path) -> Option<String>;

/// Refuses `command` with the hook's message, exiting 126 without starting it, when the session binds no hook and a decider refuses it.
pub(super) fn refusal(command: &str, screen: Screen) -> Option<ExitCode> {
    if std::env::var_os(HOOKED).is_some_and(|hooked| hooked == "1") {
        return None;
    }
    let message = screen(command, &std::env::current_dir().unwrap_or_default())?;
    eprintln!("{message}");
    Some(ExitCode::from(126))
}

/// [`refusal`] of the command a shell runs when `program` is `SHELL -c COMMAND`, as OpenCode's and Pi's shell tools start it through the session's `shell` wrapper; any other program is no shell command and passes.
pub(super) fn program_refusal(program: &[String], screen: Screen) -> Option<ExitCode> {
    match program {
        [shell, flag, command, ..] if flag == "-c" && Path::new(shell).file_name().is_some_and(|name| SHELLS.iter().any(|known| name == *known)) => refusal(command, screen),
        _ => None,
    }
}
