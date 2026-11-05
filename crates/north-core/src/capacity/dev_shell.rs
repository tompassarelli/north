//! The project flake dev shell a capacity run or a job enters, so a toolchain command never runs on a host's bare PATH, since hosts may keep toolchains only in project dev shells.
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

const MARK: &str = "NORTH_DEV_SHELL";
const USAGE: &str = "usage: north capacity dev-shell -- COMMAND...";
const EXEC_USAGE: &str = "usage: north capacity exec [--dev-shell DIRECTORY] -- COMMAND...";
const NOT_FOUND: u8 = 127;
const NOT_EXECUTABLE: u8 = 126;
const SYSTEM_NIX: [&str; 2] = ["/run/current-system/sw/bin/nix", "/nix/var/nix/profiles/default/bin/nix"];

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

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// `program` as `execvp` finds it: a path as given, else the first executable of that name on PATH.
fn resolve(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return executable(Path::new(program)).then(|| PathBuf::from(program));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|directory| directory.join(program)).find(|candidate| executable(candidate))
}

/// The nix that enters a dev shell: the one on PATH, else the system's, since a service or timer PATH may omit it.
fn nix() -> Option<PathBuf> {
    resolve("nix").or_else(|| SYSTEM_NIX.into_iter().map(PathBuf::from).find(|path| executable(path)))
}

/// `command` as a lease runs it from the current directory: through `north capacity exec`, inside its project's flake dev shell when it has one this process is not already in, unless the command is itself a `nix` invocation.
pub fn enter(north: &str, command: &[String]) -> Vec<String> {
    project(north, command).unwrap_or_else(|| bare(north, command))
}

/// `command` inside the flake dev shell of the current directory's project, when it has one this process is not already in and the command is not itself a `nix` invocation.
pub fn project(north: &str, command: &[String]) -> Option<Vec<String>> {
    let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let runs_nix = command.first().is_none_or(|program| Path::new(program).file_name().is_some_and(|name| name == "nix"));
    let (nix, shell) = (!runs_nix).then(|| project_shell(&directory)).flatten().filter(|shell| !inside(shell)).and_then(|shell| nix().zip(Some(shell)))?;
    let shell = shell.to_string_lossy().into_owned();
    let mut words = vec![nix.to_string_lossy().into_owned(), "develop".to_owned(), shell.clone(), "-c".to_owned()];
    words.extend(exec_words(north, Some(&shell)));
    words.extend_from_slice(command);
    Some(words)
}

/// `command` run through `north capacity exec` outside any dev shell, as a native client runs.
pub fn bare(north: &str, command: &[String]) -> Vec<String> {
    let mut words = exec_words(north, None);
    words.extend_from_slice(command);
    words
}

fn exec_words(north: &str, shell: Option<&str>) -> Vec<String> {
    let mut words = vec![north.to_owned(), "capacity".to_owned(), "exec".to_owned()];
    if let Some(shell) = shell {
        words.extend(["--dev-shell".to_owned(), shell.to_owned()]);
    }
    words.push("--".to_owned());
    words
}

/// `north capacity dev-shell -- COMMAND...` prints the command as it runs from the current directory, each word NUL-terminated, for a shell runner such as bin/job-run to execute.
pub fn run(arguments: &[String]) -> u8 {
    let Some(command) = arguments.split_first().filter(|(separator, command)| *separator == "--" && !command.is_empty()).map(|(_, command)| command) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let north = north_base::platform::north_binary::north_binary().to_string_lossy().into_owned();
    let mut out = std::io::stdout().lock();
    let printed = enter(&north, command).iter().try_for_each(|word| out.write_all(word.as_bytes()).and_then(|()| out.write_all(b"\0")));
    u8::from(printed.and_then(|()| out.flush()).is_err())
}

/// `north capacity exec [--dev-shell DIRECTORY] -- COMMAND...` becomes COMMAND, marking the dev shell it runs in, and exits 127 naming the program when it is not found.
pub fn exec(arguments: &[String]) -> u8 {
    let (shell, rest) = match arguments {
        [flag, shell, rest @ ..] if flag == "--dev-shell" => (Some(shell), rest),
        rest => (None, rest),
    };
    let Some((program, rest)) = rest.split_first().filter(|(separator, command)| *separator == "--" && !command.is_empty()).and_then(|(_, command)| command.split_first()) else {
        eprintln!("{EXEC_USAGE}");
        return 2;
    };
    let Some(resolved) = resolve(program) else {
        eprintln!("north capacity exec: {program}: command not found");
        return NOT_FOUND;
    };
    let mut command = std::process::Command::new(resolved);
    command.arg0(program).args(rest);
    if let Some(shell) = shell {
        command.env(MARK, shell);
    }
    let error = command.exec();
    eprintln!("north capacity exec: {program}: {error}");
    NOT_EXECUTABLE
}

const TRACE: &str = "NORTH_DEV_SHELL_TRACE";
const RUN_USAGE: &str = "usage: north dev-shell run [--dir DIRECTORY] -- COMMAND...";

/// The nearest flake at or above `directory`, stopping after the git checkout's root.
fn flake_root(directory: &Path) -> Option<PathBuf> {
    for candidate in directory.ancestors() {
        if candidate.join("flake.nix").is_file() {
            return Some(candidate.to_path_buf());
        }
        if candidate.join(".git").exists() {
            return None;
        }
    }
    None
}

fn system() -> String {
    let os = if std::env::consts::OS == "macos" { "darwin" } else { std::env::consts::OS };
    format!("{}-{os}", std::env::consts::ARCH)
}

/// The derivation path of the flake's `devShells.<system>.default`, which names the shell whatever file changed.
fn derivation(nix: &Path, flake: &Path) -> Result<String, String> {
    let attribute = format!("{}#devShells.{}.default.drvPath", flake.display(), system());
    let output = std::process::Command::new(nix).args(["eval", "--offline", "--raw", &attribute]).output().map_err(|error| format!("{}: {error}", nix.display()))?;
    let derivation = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && !derivation.is_empty() {
        return Ok(derivation);
    }
    let reason = String::from_utf8_lossy(&output.stderr).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("no output").trim().to_owned();
    Err(format!("{} has no devShells.{}.default ({reason})", flake.display(), system()))
}

fn trace(line: &str) {
    if std::env::var_os(TRACE).is_some_and(|value| !value.is_empty()) {
        eprintln!("north dev-shell: {line}");
    }
}

fn replace_with(program: &Path, name: &str, arguments: &[String], marked: Option<&str>) -> u8 {
    let mut command = std::process::Command::new(program);
    command.arg0(name).args(arguments);
    if let Some(shell) = marked {
        command.env(MARK, shell);
    }
    let error = command.exec();
    eprintln!("north dev-shell: {name}: {error}");
    NOT_EXECUTABLE
}

fn run_directly(command: &[String], marked: Option<&str>) -> u8 {
    let (program, rest) = command.split_first().expect("a non-empty command");
    match resolve(program) {
        Some(resolved) => replace_with(&resolved, program, rest, marked),
        None => {
            eprintln!("north dev-shell: {program}: command not found");
            NOT_FOUND
        }
    }
}

/// `north dev-shell run [--dir DIRECTORY] -- COMMAND...` runs COMMAND inside the flake's default dev shell, entering it once: when `NORTH_DEV_SHELL` already holds the shell's derivation path the command runs directly, and a flake without a dev shell runs it bare after saying so.
pub fn command(arguments: &[String]) -> u8 {
    let (directory, rest) = match arguments {
        [verb, flag, directory, rest @ ..] if verb == "run" && flag == "--dir" => (PathBuf::from(directory), rest),
        [verb, rest @ ..] if verb == "run" => (std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), rest),
        _ => {
            eprintln!("{RUN_USAGE}");
            return 2;
        }
    };
    let Some(command) = rest.split_first().filter(|(separator, command)| *separator == "--" && !command.is_empty()).map(|(_, command)| command) else {
        eprintln!("{RUN_USAGE}");
        return 2;
    };
    let Some(flake) = flake_root(&directory) else {
        eprintln!("north dev-shell: {} is in no flake, so {} runs outside a dev shell", directory.display(), command[0]);
        return run_directly(command, None);
    };
    let Some(nix) = nix() else {
        eprintln!("north dev-shell: nix: command not found");
        return NOT_FOUND;
    };
    let identity = match derivation(&nix, &flake) {
        Ok(identity) => identity,
        Err(reason) => {
            eprintln!("north dev-shell: {reason}, so {} runs outside a dev shell", command[0]);
            return run_directly(command, None);
        }
    };
    if std::env::var(MARK).is_ok_and(|entered| entered == identity) {
        trace(&format!("direct {identity}"));
        return run_directly(command, None);
    }
    trace(&format!("enter {identity}"));
    let mut words = vec!["develop".to_owned(), flake.to_string_lossy().into_owned(), "--offline".to_owned(), "--no-update-lock-file".to_owned(), "-c".to_owned(), "env".to_owned(), format!("{MARK}={identity}")];
    words.extend_from_slice(command);
    replace_with(&nix, "nix", &words, None)
}

