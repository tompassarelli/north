use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: north lane exec [--lane CHECKOUT] [--] PROGRAM [ARGS...]

Runs PROGRAM, a lane's own build output such as target/debug/north, outside depbox. Inside an agent's
worker sandbox, which refuses to execute anything beneath a lane's target/ or node_modules/, it runs
PROGRAM without that rule only when every lock file of CHECKOUT (default: the checkout of the current
directory) equals origin/main's, so no dependency code the landing has not admitted runs with the
agent's rights; a Cargo.lock counts as equal when it differs only in the workspace's own packages,
which carry no source, so adding a crate admits no outside code; a lane with a changed lock runs PROGRAM
through `north depbox run -- PROGRAM` instead, where the dependency code is confined.
Outside a worker sandbox, inside depbox, and with no checkout to vouch for it, it runs PROGRAM in place.";

const LOCKS: [&str; 6] = ["Cargo.lock", "bun.lock", "bun.lockb", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"];
const DEPBOX: &str = "DEPBOX";

/// `north lane exec`; `confined` says whether this command runs in a worker sandbox, and `leave` runs a program without the worker sandbox's rule against executing build output, returning its exit code, or `None` when it did not run it.
pub fn run(arguments: &[String], confined: bool, leave: fn(&[String]) -> Option<u8>) -> u8 {
    let confined = confined && std::env::var_os(DEPBOX).is_none_or(|depbox| depbox.is_empty());
    run_with(arguments, confined, leave, replace_with)
}

fn run_with(arguments: &[String], confined: bool, leave: fn(&[String]) -> Option<u8>, replace: fn(&mut Command, &str) -> u8) -> u8 {
    let (lane, program) = match arguments {
        [flag, ..] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return 0;
        }
        [flag, lane, rest @ ..] if flag == "--lane" => (Some(PathBuf::from(lane)), rest),
        rest => (None, rest),
    };
    let program = program.strip_prefix(&["--".to_owned()]).unwrap_or(program);
    if program.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let lane = match named_lane(lane, program) {
        Ok(lane) => lane,
        Err(code) => return code,
    };
    if confined && let Some(lane) = lane.or_else(|| toplevel(Path::new(".")))
        && let Some(code) = vouch(&lane, program, leave, replace)
    {
        return code;
    }
    replace(Command::new(&program[0]).args(&program[1..]), &program[0])
}

fn named_lane(lane: Option<PathBuf>, program: &[String]) -> Result<Option<PathBuf>, u8> {
    let program_checkout = std::fs::canonicalize(&program[0]).ok().filter(|_| program[0].contains('/')).and_then(|path| path.parent().and_then(toplevel));
    match (lane, program_checkout) {
        (Some(lane), Some(owner)) if std::fs::canonicalize(&lane).ok().as_ref() != Some(&owner) => {
            eprintln!("lane exec: {} is built in {}, not in {}; name that checkout with --lane or run it with `north depbox run -- {}`", program[0], owner.display(), lane.display(), program.join(" "));
            Err(1)
        }
        (Some(lane), _) => Ok(Some(lane)),
        (None, Some(owner)) => Ok(Some(owner)),
        (None, None) => Ok(None),
    }
}

/// The exit code of running `program` in a way that is safe for `lane`'s locks, or `None` when it may run in place.
fn vouch(lane: &Path, program: &[String], leave: fn(&[String]) -> Option<u8>, replace: fn(&mut Command, &str) -> u8) -> Option<u8> {
    match route(lane) {
        Ok(Route::Leave) => leave(program),
        Ok(Route::Depbox) => Some(replace(&mut depbox_command(program), "north depbox run")),
        Err(error) => {
            eprintln!("lane exec: {}: {error}; run the build output inside depbox instead: north depbox run -- {}", lane.display(), program.join(" "));
            Some(1)
        }
    }
}

enum Route {
    Leave,
    Depbox,
}

fn route(lane: &Path) -> Result<Route, String> {
    Ok(if changed_locks(lane)?.is_empty() { Route::Leave } else { Route::Depbox })
}

fn depbox_command(program: &[String]) -> Command {
    let mut command = Command::new(north_base::platform::north_binary::north_binary());
    command.args(["depbox", "run", "--"]).args(program);
    command
}

fn replace_with(command: &mut Command, name: &str) -> u8 {
    let error = command.exec();
    eprintln!("lane exec: {name}: {error}");
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}

/// The lock files of `checkout`, tracked or not, whose content differs from origin/main's.
pub fn changed_locks(checkout: &Path) -> Result<Vec<String>, String> {
    let pathspecs: Vec<String> = LOCKS.iter().map(|name| format!(":(glob)**/{name}")).collect();
    git(checkout, &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"]).map_err(|_| "no origin/main to compare its lock files with".to_owned())?;
    let mut changed = git(checkout, &[&["diff", "--name-only", "origin/main", "--"][..], &pathspecs.iter().map(String::as_str).collect::<Vec<_>>()].concat())?;
    changed.retain(|path| !only_workspace_packages_differ(checkout, path));
    changed.extend(git(checkout, &[&["ls-files", "--others", "--exclude-standard", "--"][..], &pathspecs.iter().map(String::as_str).collect::<Vec<_>>()].concat())?);
    changed.sort();
    changed.dedup();
    Ok(changed)
}

fn only_workspace_packages_differ(checkout: &Path, path: &str) -> bool {
    if !path.ends_with("Cargo.lock") {
        return false;
    }
    let (Ok(base), Ok(lane)) = (git(checkout, &["show", &format!("origin/main:{path}")]), std::fs::read_to_string(checkout.join(path))) else { return false };
    let base = base.join("\n");
    outside_packages(&base) == outside_packages(&lane) && local_packages(&lane).difference(&local_packages(&base)).all(|name| workspace_names(checkout).is_some_and(|members| members.contains(name)))
}

fn local_packages(lock: &str) -> std::collections::BTreeSet<String> {
    lock.split("[[package]]").skip(1).filter(|block| !block.lines().any(|line| line.starts_with("source = "))).filter_map(|block| block.lines().find_map(|line| line.strip_prefix("name = ")).map(|name| name.trim_matches('"').to_owned())).collect()
}

fn workspace_names(checkout: &Path) -> Option<std::collections::BTreeSet<String>> {
    let inside = std::fs::canonicalize(checkout).ok()?;
    let name_of = |directory: &Path| -> Option<String> {
        let manifest: toml_edit::DocumentMut = std::fs::read_to_string(directory.join("Cargo.toml")).ok()?.parse().ok()?;
        manifest.get("package")?.get("name")?.as_str().map(str::to_owned)
    };
    let root: toml_edit::DocumentMut = std::fs::read_to_string(inside.join("Cargo.toml")).ok()?.parse().ok()?;
    let mut names: std::collections::BTreeSet<String> = name_of(&inside).into_iter().collect();
    let members = root.get("workspace").and_then(|workspace| workspace.get("members")).and_then(toml_edit::Item::as_array);
    for member in members.into_iter().flatten() {
        let member = member.as_str()?;
        let directories: Vec<PathBuf> = match member.strip_suffix("/*") {
            Some(parent) if !parent.contains('*') => std::fs::read_dir(inside.join(parent)).ok()?.flatten().map(|entry| entry.path()).collect(),
            _ if member.contains('*') => return None,
            _ => vec![inside.join(member)],
        };
        for directory in directories {
            let Ok(directory) = std::fs::canonicalize(directory) else { continue };
            if !directory.starts_with(&inside) {
                return None;
            }
            names.extend(name_of(&directory));
        }
    }
    Some(names)
}

fn outside_packages(lock: &str) -> std::collections::BTreeSet<(String, String, String, String)> {
    let field = |block: &str, key: &str| block.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(" = ")).unwrap_or_default().to_owned();
    lock.split("[[package]]").skip(1).filter_map(|block| {
        let source = field(block, "source");
        (!source.is_empty()).then(|| (field(block, "name"), field(block, "version"), source, field(block, "checksum")))
    }).collect()
}

fn toplevel(directory: &Path) -> Option<PathBuf> {
    git(directory, &["rev-parse", "--show-toplevel"]).ok()?.into_iter().next().map(PathBuf::from)
}

fn git(directory: &Path, arguments: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {} exited {}: {}", arguments.join(" "), output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
