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
    Some(through(north, &nix, &shell.to_string_lossy(), command))
}

/// `command` inside `nix develop SHELL`, re-entering `north capacity exec --dev-shell SHELL` there, which marks the shell once it is entered.
fn through(north: &str, nix: &Path, shell: &str, command: &[String]) -> Vec<String> {
    let mut words = vec![nix.to_string_lossy().into_owned(), "develop".to_owned(), shell.to_owned(), "-c".to_owned()];
    words.extend(exec_words(north, Some(shell)));
    words.extend_from_slice(command);
    words
}

fn north_binary() -> String {
    std::env::var("AGENT_CAPACITY_NORTH").ok().filter(|path| !path.is_empty()).or_else(|| std::env::current_exe().ok().map(|path| path.to_string_lossy().into_owned())).unwrap_or_else(|| "north".to_owned())
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
    let north = std::env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.to_string_lossy().into_owned());
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
    if let Some((nix, shell)) = shell.filter(|shell| !inside(Path::new(shell))).and_then(|shell| nix().zip(Some(shell))) {
        let command: Vec<String> = std::iter::once(program.clone()).chain(rest.iter().cloned()).collect();
        let words = through(&north_binary(), &nix, shell, &command);
        let error = std::process::Command::new(&words[0]).args(&words[1..]).exec();
        eprintln!("north capacity exec: {}: {error}", words[0]);
        return NOT_EXECUTABLE;
    }
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::*;

    const CHILD: &str = "capacity::dev_shell::tests::exec_child";
    const ARGS: &str = "NORTH_TEST_EXEC_ARGS";

    fn script(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The `exec` verb run for real in a child of the test binary, since it replaces its process.
    #[test]
    fn exec_child() {
        if let Ok(arguments) = std::env::var(ARGS) {
            let arguments: Vec<String> = arguments.split('\u{1f}').map(str::to_owned).collect();
            std::process::exit(i32::from(exec(&arguments)));
        }
    }

    struct Fixture {
        _scratch: tempfile::TempDir,
        shell: PathBuf,
        path: std::ffi::OsString,
        north: PathBuf,
    }

    fn fixture() -> Fixture {
        let scratch = tempfile::tempdir().unwrap();
        let shell = scratch.path().join("project");
        script(&shell.join("tools/only-in-shell"), "echo only-in-shell ran $*");
        let nix = scratch.path().join("bin/nix");
        script(&nix, "[ \"$1\" = develop ] && [ \"$3\" = -c ] || exit 64\nshell=$2; shift 3\nPATH=\"$shell/tools:$PATH\" IN_NIX_SHELL=impure exec \"$@\"");
        let north = scratch.path().join("bin/north");
        script(&north, "[ \"$1 $2 $3 $5\" = \"capacity exec --dev-shell --\" ] || exit 65\nshift 5\nexec \"$@\"");
        let path = std::env::join_paths([scratch.path().join("bin"), PathBuf::from("/usr/bin"), PathBuf::from("/bin")]).unwrap();
        Fixture { _scratch: scratch, shell, path, north }
    }

    fn exec_run(fixture: &Fixture, arguments: &[&str], environment: &[(&str, &str)]) -> (i32, String, String) {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", CHILD, "--nocapture"]).env(ARGS, arguments.join("\u{1f}")).env("PATH", &fixture.path).env("AGENT_CAPACITY_NORTH", &fixture.north).env_remove("IN_NIX_SHELL").env_remove(MARK);
        child.envs(environment.iter().copied());
        let output = child.output().unwrap();
        (output.status.code().unwrap(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    #[test]
    fn capacity_dev_shell_exec_with_a_dev_shell_runs_a_program_found_only_inside_that_shell() {
        let fixture = fixture();
        let shell = fixture.shell.to_str().unwrap();
        let (code, out, err) = exec_run(&fixture, &["--dev-shell", shell, "--", "only-in-shell", "--version"], &[]);
        assert_eq!((code, out.contains("only-in-shell ran --version")), (0, true), "{out}{err}");
        let (code, _, err) = exec_run(&fixture, &["--", "only-in-shell"], &[]);
        assert_eq!((code, err.contains("command not found")), (127, true), "{err}");
    }

    #[test]
    fn capacity_dev_shell_exec_inside_the_shell_runs_the_program_without_entering_again() {
        let fixture = fixture();
        let shell = fixture.shell.to_str().unwrap();
        let tools = format!("{}:{}", fixture.shell.join("tools").display(), fixture.path.to_str().unwrap());
        let (code, out, err) = exec_run(&fixture, &["--dev-shell", shell, "--", "only-in-shell"], &[("IN_NIX_SHELL", "impure"), ("PATH", &tools)]);
        assert_eq!((code, out.contains("only-in-shell ran")), (0, true), "{out}{err}");
    }
}
