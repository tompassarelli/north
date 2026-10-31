//! The project flake dev shell a capacity run or a job enters, so a toolchain command never runs on a host's bare PATH, since hosts may keep toolchains only in project dev shells.
use std::io::Write;
use std::path::{Path, PathBuf};

const MARK: &str = "NORTH_DEV_SHELL";
const USAGE: &str = "usage: north capacity dev-shell -- COMMAND...";

/// The flake at or above `directory`, inside its git checkout, that declares a dev shell.
fn project_shell(directory: &Path) -> Option<PathBuf> {
    for candidate in directory.ancestors() {
        if std::fs::read_to_string(candidate.join("flake.nix")).is_ok_and(|flake| flake.contains("devShell")) {
            return Some(candidate.to_path_buf());
        }
        if candidate.join(".git").exists() {
            return None;
        }
    }
    None
}

/// A shell this process already runs in: the one north entered, else any `nix develop` or `nix-shell` it was started from.
fn inside(shell: &Path) -> bool {
    match std::env::var_os(MARK) {
        Some(entered) => Path::new(&entered) == shell,
        None => std::env::var_os("IN_NIX_SHELL").is_some_and(|value| !value.is_empty()),
    }
}

fn nix_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|directory| directory.join("nix").is_file()))
}

/// `command` as run from the current directory: inside its project's flake dev shell when it has one this process is not already in, unless the command is itself a `nix` invocation.
pub fn enter(command: &[String]) -> Vec<String> {
    let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let runs_nix = command.first().is_none_or(|program| Path::new(program).file_name().is_some_and(|name| name == "nix"));
    let Some(shell) = (!runs_nix).then(|| project_shell(&directory)).flatten().filter(|shell| !inside(shell) && nix_on_path()) else {
        return command.to_vec();
    };
    let shell = shell.to_string_lossy().into_owned();
    let mut entered = vec!["nix".to_owned(), "develop".to_owned(), shell.clone(), "-c".to_owned(), "env".to_owned(), format!("{MARK}={shell}")];
    entered.extend_from_slice(command);
    entered
}

/// `north capacity dev-shell -- COMMAND...` prints the command as it runs from the current directory, each word NUL-terminated, for a shell runner such as bin/job-run to execute.
pub fn run(arguments: &[String]) -> u8 {
    let Some(command) = arguments.split_first().filter(|(separator, command)| *separator == "--" && !command.is_empty()).map(|(_, command)| command) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut out = std::io::stdout().lock();
    let printed = enter(command).iter().try_for_each(|word| out.write_all(word.as_bytes()).and_then(|()| out.write_all(b"\0")));
    u8::from(printed.and_then(|()| out.flush()).is_err())
}
