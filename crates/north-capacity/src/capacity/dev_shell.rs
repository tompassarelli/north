//! The project flake dev shell a capacity run or a job enters, so a toolchain command never runs on a host's bare PATH, since hosts may keep toolchains only in project dev shells.
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const MARK: &str = "NORTH_DEV_SHELL";
const USAGE: &str = "usage: north capacity dev-shell -- COMMAND...";
const EXEC_USAGE: &str = "usage: north capacity exec [--dev-shell DIRECTORY] -- COMMAND...";
const RUN_USAGE: &str = "usage: north capacity dev-shell run [--dir DIRECTORY] (--prepare | -- COMMAND...)";
const TRACE: &str = "NORTH_DEV_SHELL_TRACE";
pub(crate) const NOT_FOUND: u8 = 127;
pub(crate) const NOT_EXECUTABLE: u8 = 126;
const SEED_FAILED: u8 = 125;
const NOT_A_STORE_CLIENT: u8 = 78;
const SYSTEM_NIX: [&str; 2] = ["/run/current-system/sw/bin/nix", "/nix/var/nix/profiles/default/bin/nix"];
const PREPARE_TIMEOUT_SECONDS: &str = "1800";
const WORKTREES: &str = ".worktrees";
const DERIVATIONS: &str = ".derivations";
const DERIVATION_HOLD: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

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

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").arg("--no-optional-locks").arg("-C").arg(directory).args(arguments).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Where a clean checkout's dev shell derivation is kept: a git flake evaluates only tracked files, so the tree, the flake's place in it, the nix and the system name the derivation, and every slot or worktree on that tree shares one evaluation.
fn remembered(nix: &Path, flake: &Path) -> Option<PathBuf> {
    if !git(flake, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        return None;
    }
    let place = git(flake, &["rev-parse", "HEAD^{tree}", "--show-prefix"])?;
    let mut digest = Sha256::new();
    for part in [nix.as_os_str().as_encoded_bytes(), system().as_bytes(), place.as_bytes()] {
        digest.update(part);
        digest.update([0]);
    }
    Some(roots().join(DERIVATIONS).join(digest.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect::<String>()))
}

fn remember(entry: &Path, derivation: &str) {
    let Some(directory) = entry.parent() else { return };
    let part = entry.with_extension(format!("part{}", std::process::id()));
    let written = std::fs::create_dir_all(directory).and_then(|()| std::fs::write(&part, derivation)).and_then(|()| std::fs::rename(&part, entry));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&part);
        trace(&format!("cannot remember {derivation} at {}: {error}", entry.display()));
        return;
    }
    let stale = std::time::SystemTime::now() - DERIVATION_HOLD;
    for old in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        if old.metadata().and_then(|metadata| metadata.modified()).is_ok_and(|modified| modified < stale) {
            let _ = std::fs::remove_file(old.path());
        }
    }
}

/// The derivation path of the flake's `devShells.<system>.default`, which names the shell whatever file changed; a clean checkout reuses the path its tree last evaluated to while that derivation is still in the store.
fn derivation(nix: &Path, flake: &Path) -> Result<String, String> {
    let entry = remembered(nix, flake);
    if let Some(known) = entry.as_deref().and_then(|entry| std::fs::read_to_string(entry).ok()).filter(|known| !known.is_empty() && Path::new(known).exists()) {
        return Ok(known);
    }
    super::subject_start::step("nix eval");
    let derivation = evaluate(nix, flake)?;
    if let Some(entry) = entry {
        remember(&entry, &derivation);
    }
    Ok(derivation)
}

fn evaluate(nix: &Path, flake: &Path) -> Result<String, String> {
    let attribute = format!("{}#devShells.{}.default.drvPath", flake.display(), system());
    let output = std::process::Command::new(nix).args(["eval", "--offline", "--raw", &attribute]).output().map_err(|error| format!("{}: {error}", nix.display()))?;
    let derivation = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && !derivation.is_empty() {
        return Ok(derivation);
    }
    let reason = String::from_utf8_lossy(&output.stderr).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("no output").trim().to_owned();
    Err(format!("{} has no devShells.{}.default ({reason})", flake.display(), system()))
}

/// Whether the checkout at `flake` holds what the shell's hook leaves in it: the link to the Buck2 toolchain file (flake.nix), which a Buck2 checkout made after this process entered the shell lacks.
fn hooked(flake: &Path) -> bool {
    !flake.join(".buckconfig").is_file() || flake.join(".buckconfig.flake").exists()
}

/// The dev shell this process runs in only when north entered that very derivation (`entered`, the mark) and its hook has run for the checkout at `flake`; a stray `IN_NIX_SHELL` from another shell does not count.
fn inside(entered: Option<&str>, identity: &str, flake: &Path) -> bool {
    entered == Some(identity) && hooked(flake)
}

fn entered() -> Option<String> {
    std::env::var(MARK).ok()
}

/// `command` as a lease runs it from the current directory: inside its project's flake dev shell through `north capacity dev-shell run`, unless it has no flake or the command is itself a `nix` invocation.
pub fn enter(north: &str, command: &[String]) -> Vec<String> {
    let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    if runs_nix(command) || flake_root(&directory).is_none() {
        return bare(north, command);
    }
    let mut words = vec![north.to_owned(), "capacity".to_owned(), "dev-shell".to_owned(), "run".to_owned(), "--".to_owned()];
    words.extend_from_slice(command);
    words
}

fn runs_nix(command: &[String]) -> bool {
    command.first().is_none_or(|program| Path::new(program).file_name().is_some_and(|name| name == "nix")) || super::sandbox::sandboxes(command)
}

/// The nix, flake and derivation of the dev shell `command` would enter from the current directory: none when the directory has no flake with a `devShells` output, the command is itself a `nix` invocation or this process is already in that shell.
fn entered_by(command: &[String]) -> Option<(PathBuf, PathBuf, String)> {
    let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let flake = (!runs_nix(command)).then(|| flake_root(&directory)).flatten()?;
    let nix = nix()?;
    let identity = derivation(&nix, &flake).ok()?;
    (!inside(entered().as_deref(), &identity, &flake)).then_some((nix, flake, identity))
}

/// Makes the root of the dev shell `command` would enter from the current directory, from where the roots directory can be written, so a sandbox that binds it read-only finds the root there; 0 when the command enters no dev shell.
pub fn prepare_root(command: &[String]) -> u8 {
    let Some((nix, flake, identity)) = entered_by(command) else { return 0 };
    ensure_root(&nix, &flake, &identity, &roots(), leased()).err().unwrap_or(0)
}

/// `command` inside the current directory's project dev shell, when its flake has a `devShells` output this process is not already in and the command is not itself a `nix` invocation.
pub fn project(north: &str, command: &[String]) -> Option<Vec<String>> {
    let (_, flake, _) = entered_by(command)?;
    let mut words = vec![north.to_owned(), "capacity".to_owned(), "dev-shell".to_owned(), "run".to_owned(), "--dir".to_owned(), flake.to_string_lossy().into_owned(), "--".to_owned()];
    words.extend_from_slice(command);
    Some(words)
}

/// `command` run through `north capacity exec` outside any dev shell, as a native client runs.
pub fn bare(north: &str, command: &[String]) -> Vec<String> {
    let mut words = vec![north.to_owned(), "capacity".to_owned(), "exec".to_owned(), "--".to_owned()];
    words.extend_from_slice(command);
    words
}

/// `north capacity dev-shell -- COMMAND...` prints the command as it runs from the current directory, each word NUL-terminated, for a shell runner such as `north job-run` to execute.
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

/// `north capacity exec [--dev-shell DIRECTORY] -- COMMAND...` becomes COMMAND, inside DIRECTORY's dev shell through `north capacity dev-shell run` when one is named, and exits 127 naming the program when it is not found; `gated` is the capacity gate's word on whether it leases a command, as `command` takes it.
pub fn exec(arguments: &[String], gated: &dyn Fn(&[String]) -> bool) -> u8 {
    if let [flag, directory, rest @ ..] = arguments
        && flag == "--dev-shell"
    {
        let mut run = vec!["run".to_owned(), "--dir".to_owned(), directory.clone()];
        run.extend_from_slice(rest);
        return command(&run, gated);
    }
    let Some(command) = arguments.split_first().filter(|(separator, command)| *separator == "--" && !command.is_empty()).map(|(_, command)| command) else {
        eprintln!("{EXEC_USAGE}");
        return 2;
    };
    run_directly("north capacity exec", command)
}

fn seed_target(flake: &Path) -> Result<(), String> {
    let north = north_base::platform::north_binary::north_binary();
    let status = std::process::Command::new(&north).args(["lane", "seed"]).arg(flake).status().map_err(|error| format!("{}: {error}", north.display()))?;
    if status.success() { Ok(()) } else { Err(format!("north lane seed {} failed ({status}), so this shell would compile cold", flake.display())) }
}

fn trace(line: &str) {
    if std::env::var_os(TRACE).is_some_and(|value| !value.is_empty()) {
        eprintln!("north capacity dev-shell: {line}");
    }
}

fn replace_with(who: &str, command: &mut std::process::Command, name: &str) -> u8 {
    let error = command.exec();
    eprintln!("{who}: {name}: {error}");
    NOT_EXECUTABLE
}

fn run_directly(who: &str, command: &[String]) -> u8 {
    if let Some(refusal) = super::tool_homes::refusal(who) {
        eprintln!("{refusal}");
        return NOT_EXECUTABLE;
    }
    let (program, rest) = command.split_first().expect("a non-empty command");
    match resolve(program) {
        Some(resolved) => {
            let started = super::subject_start::mark();
            let code = replace_with(who, std::process::Command::new(&resolved).arg0(program).args(rest).env_remove(super::subject_start::ENV), program);
            super::subject_start::unmark(started);
            code
        }
        None => {
            eprintln!("{who}: {program}: command not found");
            NOT_FOUND
        }
    }
}

/// Where each dev shell's GC root lives, one directory per derivation.
fn roots() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    state.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")).join("north/dev-shells")
}

/// The root's name: the derivation's store name, so an edit to any file the shell's derivation reads gets a new root.
fn root_key(identity: &str) -> String {
    let name = Path::new(identity).file_name().map_or_else(|| identity.to_owned(), |name| name.to_string_lossy().into_owned());
    name.strip_suffix(".drv").map_or_else(|| name.clone(), str::to_owned)
}

fn worktree_pointer(roots: &Path, flake: &Path) -> PathBuf {
    let digest = Sha256::digest(flake.as_os_str().as_encoded_bytes());
    roots.join(WORKTREES).join(digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect::<String>())
}

/// Records that `flake` produces `key`, so the root lives while a worktree whose flake last produced it exists.
fn point(roots: &Path, flake: &Path, key: &str) {
    let pointer = worktree_pointer(roots, flake);
    let content = format!("{key}\n{}\n", flake.display());
    if std::fs::read_to_string(&pointer).is_ok_and(|current| current == content) {
        return;
    }
    let written = pointer.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&pointer, content));
    if let Err(error) = written {
        trace(&format!("cannot record {} for {}: {error}", key, flake.display()));
    }
}

/// Drops every pointer whose worktree is gone and every root no remaining worktree produces.
fn sweep(roots: &Path, keep: &str) {
    let pointers = roots.join(WORKTREES);
    let mut live = std::collections::BTreeSet::from([keep.to_owned()]);
    for pointer in std::fs::read_dir(&pointers).into_iter().flatten().flatten().map(|entry| entry.path()) {
        let content = std::fs::read_to_string(&pointer).unwrap_or_default();
        let mut lines = content.lines();
        match (lines.next(), lines.next()) {
            (Some(key), Some(flake)) if Path::new(flake).join("flake.nix").is_file() => {
                live.insert(key.to_owned());
            }
            _ => {
                let _ = std::fs::remove_file(&pointer);
            }
        }
    }
    for entry in std::fs::read_dir(roots).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && !live.contains(&name) && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            trace(&format!("drop {name}"));
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn leased() -> bool {
    std::env::var_os("AGENT_CAPACITY_LEASE").is_some_and(|lease| !lease.is_empty())
}

/// The profile that roots the shell of `identity` under `roots`, realised first when it does not exist yet.
pub fn ensure_root(nix: &Path, flake: &Path, identity: &str, roots: &Path, leased: bool) -> Result<PathBuf, u8> {
    let key = root_key(identity);
    let profile = roots.join(&key).join("profile");
    point(roots, flake, &key);
    if !profile.exists() {
        prepare(nix, flake, &profile, &key, leased)?;
        sweep(roots, &key);
    }
    Ok(profile)
}

/// Realises the shell's environment once with the network, under a capacity lease unless this process already holds one (`leased`), and roots it at `profile`.
fn prepare(nix: &Path, flake: &Path, profile: &Path, key: &str, leased: bool) -> Result<(), u8> {
    std::fs::create_dir_all(profile.parent().expect("a root directory")).map_err(|error| {
        eprintln!("north capacity dev-shell: {}: {error}", profile.display());
        1u8
    })?;
    let realise = [nix.to_string_lossy().into_owned(), "print-dev-env".to_owned(), flake.to_string_lossy().into_owned(), "--no-update-lock-file".to_owned(), "--profile".to_owned(), profile.to_string_lossy().into_owned()];
    let mut command = if leased {
        let mut command = std::process::Command::new(&realise[0]);
        command.args(&realise[1..]);
        command
    } else {
        let mut command = std::process::Command::new(north_base::platform::north_binary::north_binary());
        command.args(["capacity", "host", "run", "--class", "moderate", "--owner", &format!("dev-shell:{key}"), "--timeout-seconds", PREPARE_TIMEOUT_SECONDS, "--"]).args(&realise);
        command
    };
    trace(&format!("root {key}"));
    let status = command.stdout(std::process::Stdio::null()).status().map_err(|error| {
        eprintln!("north capacity dev-shell: cannot realise {}: {error}", flake.display());
        1u8
    })?;
    if status.success() && profile.exists() {
        return Ok(());
    }
    eprintln!("north capacity dev-shell: realising the dev shell of {} with the network failed ({status}); run `north capacity dev-shell run --prepare` outside any sandbox", flake.display());
    Err(u8::try_from(status.code().unwrap_or(1)).ok().filter(|code| *code != 0).unwrap_or(1))
}

struct Invocation {
    directory: PathBuf,
    prepare_only: bool,
    command: Vec<String>,
}

fn parse(arguments: &[String]) -> Option<Invocation> {
    let (verb, mut rest) = arguments.split_first()?;
    if verb != "run" {
        return None;
    }
    let mut invocation = Invocation { directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), prepare_only: false, command: Vec::new() };
    loop {
        match rest {
            [flag, directory, tail @ ..] if flag == "--dir" => {
                invocation.directory = PathBuf::from(directory);
                rest = tail;
            }
            [flag, tail @ ..] if flag == "--prepare" => {
                invocation.prepare_only = true;
                rest = tail;
            }
            [] if invocation.prepare_only => return Some(invocation),
            [separator, command @ ..] if separator == "--" && !command.is_empty() && !invocation.prepare_only => {
                invocation.command = command.to_vec();
                return Some(invocation);
            }
            _ => return None,
        }
    }
}

/// The capacity gate's command for a run the gate leases (`gated`), which must not enter a dev shell here first: inside it the shell's own toolchain comes before the gate's shims on PATH, so a fleet session's compile would run on this host unleased. The gate offers it to the build hosts first and otherwise leases it here, and that lease enters the dev shell. The gate gets the bare command when its dev shell is the current directory's (`own_shell`), which the lease and every build host enter themselves, else the whole run.
fn placed(north: &str, invocation: &Invocation, own_shell: bool, gated: &dyn Fn(&[String]) -> bool) -> Option<Vec<String>> {
    if invocation.prepare_only || !gated(&invocation.command) {
        return None;
    }
    let mut words = vec![north.to_owned(), "capacity".to_owned(), "gate".to_owned()];
    if !own_shell {
        words.extend([north.to_owned(), "capacity".to_owned(), "dev-shell".to_owned(), "run".to_owned(), "--dir".to_owned(), invocation.directory.to_string_lossy().into_owned(), "--".to_owned()]);
    }
    words.extend(invocation.command.iter().cloned());
    Some(words)
}

/// `north capacity dev-shell run [--dir DIRECTORY] (--prepare | -- COMMAND...)` runs COMMAND inside the flake's default dev shell, once the capacity gate has seen it (`gated` says whether the gate leases a command, `placed`), entering it once: when `NORTH_DEV_SHELL` already holds the shell's derivation path the command runs directly, otherwise it enters offline from the shell's GC root, realising that root first with the network when it is missing. `--prepare` only realises the root. A flake without a dev shell runs COMMAND bare after saying so.
pub fn command(arguments: &[String], gated: &dyn Fn(&[String]) -> bool) -> u8 {
    let Some(invocation) = parse(arguments) else {
        eprintln!("{RUN_USAGE}");
        return 2;
    };
    if let Some(refusal) = super::store_client::refusal_here(&invocation.command, &flake_root(&invocation.directory).unwrap_or_else(|| invocation.directory.clone())) {
        eprintln!("{refusal}");
        return NOT_A_STORE_CLIENT;
    }
    let north = north_base::platform::north_binary::north_binary().to_string_lossy().into_owned();
    let shell_of = |directory: &Path| std::fs::canonicalize(directory).ok().and_then(|directory| flake_root(&directory));
    let own_shell = std::env::current_dir().ok().and_then(|directory| shell_of(&directory)) == shell_of(&invocation.directory);
    if let Some(words) = placed(&north, &invocation, own_shell, gated) {
        trace("place through the capacity gate");
        super::subject_start::step("capacity gate");
        return replace_with("north capacity dev-shell", std::process::Command::new(&words[0]).args(&words[1..]), &words[0]);
    }
    let what = invocation.command.first().map_or_else(|| "nothing".to_owned(), Clone::clone);
    let bare = |reason: String| {
        eprintln!("north capacity dev-shell: {reason}, so {what} runs outside a dev shell");
        if invocation.prepare_only { 0 } else { run_directly("north capacity dev-shell", &invocation.command) }
    };
    let Some(flake) = flake_root(&invocation.directory) else {
        return bare(format!("{} is in no flake", invocation.directory.display()));
    };
    let Some(nix) = nix() else {
        eprintln!("north capacity dev-shell: nix: command not found");
        return NOT_FOUND;
    };
    super::subject_start::step("derivation");
    let identity = match derivation(&nix, &flake) {
        Ok(identity) => identity,
        Err(reason) => return bare(reason),
    };
    if !invocation.prepare_only && inside(entered().as_deref(), &identity, &flake) {
        trace(&format!("direct {identity}"));
        return run_directly("north capacity dev-shell", &invocation.command);
    }
    super::subject_start::step("gc root");
    let profile = match ensure_root(&nix, &flake, &identity, &roots(), leased()) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    if invocation.prepare_only {
        return 0;
    }
    super::subject_start::step("target seed");
    if let Err(reason) = seed_target(&flake) {
        eprintln!("north capacity dev-shell: {reason}");
        return SEED_FAILED;
    }
    trace(&format!("enter {identity}"));
    super::subject_start::step("nix develop");
    let mut words = vec!["develop".to_owned(), profile.to_string_lossy().into_owned(), "--offline".to_owned(), "-c".to_owned(), "env".to_owned(), format!("{MARK}={identity}"), north, "capacity".to_owned(), "exec".to_owned(), "--".to_owned()];
    words.extend(invocation.command);
    replace_with("north capacity dev-shell", std::process::Command::new(&nix).arg0("nix").args(&words), "nix")
}

#[cfg(test)]
#[path = "dev_shell_tests.rs"]
mod tests;
