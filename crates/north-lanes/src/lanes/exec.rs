use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: north lane exec [--lane CHECKOUT] [--] PROGRAM [ARGS...]

Runs PROGRAM, a lane's own build output such as target/debug/north, outside depbox. Inside an agent's
worker sandbox, which refuses to execute anything beneath a lane's target/ or node_modules/, it runs
PROGRAM without that rule only when every lock file of CHECKOUT (default: the checkout of the current
directory) equals origin/main's, so no dependency code the landing has not admitted runs with the
agent's rights; a Cargo.lock counts as equal when it differs only in the workspace's own packages,
which carry no source, so adding a crate admits no outside code; a lane with a changed lock runs its build
output with `north depbox run -- PROGRAM`.
Outside a worker sandbox, inside depbox, and with no checkout to vouch for it, it runs PROGRAM in place.";

const LOCKS: [&str; 6] = ["Cargo.lock", "bun.lock", "bun.lockb", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"];
const DEPBOX: &str = "DEPBOX";

/// `north lane exec`; `confined` says whether this command runs in a worker sandbox, and `leave` runs a program without the worker sandbox's rule against executing build output, returning its exit code, or `None` when it did not run it.
pub fn run(arguments: &[String], confined: bool, leave: fn(&[String]) -> Option<u8>) -> u8 {
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
    let confined = confined && std::env::var_os(DEPBOX).is_none_or(|depbox| depbox.is_empty());
    if confined && let Some(lane) = lane.or_else(|| toplevel(Path::new("."))) {
        match changed_locks(&lane) {
            Ok(changed) if changed.is_empty() => {}
            Ok(changed) => {
                eprintln!("lane exec: {} changes {} against origin/main, so its build output may carry dependency code no landing admitted; run it inside depbox instead: north depbox run -- {}", lane.display(), changed.join(", "), program.join(" "));
                return 1;
            }
            Err(error) => {
                eprintln!("lane exec: {}: {error}; run the build output inside depbox instead: north depbox run -- {}", lane.display(), program.join(" "));
                return 1;
            }
        }
        if let Some(code) = leave(program) {
            return code;
        }
    }
    let error = Command::new(&program[0]).args(&program[1..]).exec();
    eprintln!("lane exec: {}: {error}", program[0]);
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
    outside_packages(&base.join("\n")) == outside_packages(&lane)
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
