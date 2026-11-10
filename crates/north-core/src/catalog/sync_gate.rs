use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::NorthResult;
use crate::platform::{git, harness, shared_config};

use super::{home, operator_dir, repo_root};

const NOT_PUBLISHED: &str = "agents sync: generation not published";

pub fn operator_directory() -> NorthResult<PathBuf> {
    operator_dir(env::var_os("AGENTS_OPERATOR_DIR"), repo_root)
}

const SIBLING_PROJECTS: [&str; 2] = ["wisp", "makgora"];

pub fn run(arguments: &[String]) -> NorthResult<u8> {
    let operator = operator_directory()?;
    let mut say = |line: String| eprintln!("{line}");
    let now = crate::platform::records_mirror::now() as u64;
    let south = toplevel(&operator);
    let shared = match &south {
        Some(south) => shared_configuration(&home()?, south, now, &mut say),
        None => Ok(()),
    };
    let projected = shared.is_ok() && project_profiles();
    if let Err(line) = shared {
        eprintln!("{line}");
        eprintln!("{NOT_PUBLISHED}");
        return Ok(1);
    }
    let Some(south) = south else {
        eprintln!("agents sync: operator directory is not in a Git checkout");
        return Ok(1);
    };
    let north = north_checkout(
        env::var("NORTH_REPO_ROOTS").ok().as_deref(),
        env::var_os("NORTH_HOME"),
        &home()?,
    )?;
    let nixos_config = repo_root("nixos-config")?;
    let current = current_main("south", &south, now, &mut say)
        .and_then(|()| current_main("north", &north, now, &mut say))
        .and_then(|()| current_main("nixos-config", &nixos_config, now, &mut say));
    if let Err(line) = current {
        eprintln!("{line}");
        eprintln!("{NOT_PUBLISHED}");
        return Ok(1);
    }
    for project in SIBLING_PROJECTS {
        if let Err(line) = repo_root(project).map_err(|error| error.to_string()).and_then(|directory| current_main(project, &directory, now, &mut say)) {
            say(format!("agents sync: {line}; its goals are read from the checkout as it stands"));
        }
    }
    let harness = harness::harness_root().map_err(crate::error::NorthError::Configuration)?;
    if !policy_gate(&nixos_config, &operator) {
        eprintln!("{NOT_PUBLISHED}; the last published generation stays live");
        return Ok(north_base::agents_sync::POLICY_HELD);
    }
    super::run(arguments)?;
    if !projected {
        return Ok(1);
    }
    let _ = Command::new(harness.join("bin/effect-kit"))
        .arg("scan")
        .status();
    Ok(0)
}

const SYNC_USAGE: &str = "usage: north lane sync CHECKOUT...

Brings each CHECKOUT, the absolute path of a project's main checkout (.../PROJECT/main), up to origin/main by the fetch and fast-forward `agents sync` makes, clearing untracked files whose bytes equal the incoming blob. A sandboxed worker runs it through the broker, since the fast-forward unlinks and replaces files its own domain refuses to remove. A checkout that is off main, ahead of origin, or holds uncommitted changes or differing untracked files is left as it is and the reason named.";

pub fn sync_checkouts(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{SYNC_USAGE}");
        return 0;
    }
    if arguments.is_empty() || arguments.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("{SYNC_USAGE}");
        return 2;
    }
    let now = crate::platform::records_mirror::now() as u64;
    let mut say = |line: String| eprintln!("{line}");
    let mut code = 0;
    for checkout in arguments.iter().map(PathBuf::from) {
        match main_checkout_name(&checkout).and_then(|name| current_main(&name, &checkout, now, &mut say).map(|()| name)) {
            Ok(name) => println!("{name} main is at origin/main"),
            Err(reason) => {
                eprintln!("kept {}: {reason}", checkout.display());
                code = 1;
            }
        }
    }
    code
}

fn main_checkout_name(checkout: &Path) -> Result<String, String> {
    let name = checkout
        .parent()
        .and_then(Path::file_name)
        .filter(|_| checkout.is_absolute() && checkout.file_name().is_some_and(|name| name == "main") && checkout.join(".git").is_dir())
        .ok_or("name the main checkout of a project by its absolute path (.../PROJECT/main with a .git directory)")?;
    Ok(name.to_string_lossy().into_owned())
}

fn project_profiles() -> bool {
    let cfg = crate::usage::profiles::policy_value(&crate::platform::orchestration::path());
    match crate::usage::profiles::sync(&cfg, &crate::platform::host::name(), &crate::usage::profiles::home()) {
        Ok(lines) => {
            lines.iter().for_each(|line| println!("{line}"));
            true
        }
        Err(error) => {
            eprintln!("Claude profile sync: {error}");
            false
        }
    }
}

fn toplevel(directory: &Path) -> Option<PathBuf> {
    let output = git::run(directory, &["rev-parse", "--show-toplevel"]).ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

fn policy_gate(nixos_config: &Path, operator: &Path) -> bool {
    crate::agent_config_check::policy_gate_session(nixos_config, operator)
}

pub(super) fn current_main(
    name: &str,
    directory: &Path,
    now: u64,
    say: &mut impl FnMut(String),
) -> Result<(), String> {
    let git = |args: &[&str]| {
        git::run(directory, args)
            .ok()
            .filter(|output| output.status.success())
    };
    let count = |args: &[&str]| {
        git(args).and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
    };
    match north_git_locks::clear(directory) {
        Ok(sweep) => sweep.removed.iter().for_each(|lock| say(format!("agents sync: {name} cleared the stale git lock {}", lock.display()))),
        Err(error) => say(format!("agents sync: {name} could not clear stale git locks: {error}")),
    }
    let fetched = git(&["fetch", "-q", "origin", "main"]).is_some();
    let mut behind = 0;
    if fetched {
        stamp_fetch(directory, now);
        behind = count(&["rev-list", "--count", "HEAD..origin/main"])
            .ok_or_else(|| format!("{name} main: cannot count commits behind origin"))?;
        if behind == 0 {
            return Ok(());
        }
    } else {
        let age = last_fetch(directory).map(|at| now.saturating_sub(at));
        match age.filter(|age| *age <= shared_config::BOUND_SECONDS) {
            Some(age) => say(format!(
                "agents sync: {name} fetch failed; using local main, last fetched {} min ago",
                age / 60
            )),
            None => {
                return Err(format!(
                    "{name} main refused offline: its last fetch of origin/main is {}, past the {} min bound",
                    age.map_or_else(|| "unknown".to_owned(), |age| format!("{} min old", age / 60)),
                    shared_config::BOUND_SECONDS / 60
                ));
            }
        }
    }
    let ahead = count(&["rev-list", "--count", "origin/main..HEAD"]).unwrap_or(0);
    let branch = git(&["symbolic-ref", "-q", "--short", "HEAD"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|output| !output.stdout.is_empty());
    let reason = match (branch.as_deref(), ahead, dirty) {
        (branch, 0, false) if branch != Some("main") => match reattach(&git) {
            Ok(()) => {
                let left = branch.map_or_else(|| "detached".to_owned(), |branch| format!("on branch {branch}"));
                say(format!(
                    "agents sync: {name} checkout was {left} at an ancestor of origin/main; switched to main"
                ));
                fast_forward(directory, fetched, &git)
            }
            Err(reason) => Some(reason),
        },
        (branch, _, _) if branch != Some("main") => Some("checkout is not on main".to_owned()),
        (_, ahead, _) if ahead > 0 => Some(format!("diverged with {ahead} local commits")),
        (_, _, true) => Some("uncommitted changes".to_owned()),
        _ => fast_forward(directory, fetched, &git),
    };
    let Some(reason) = reason else {
        return Ok(());
    };
    Err(match fetched {
        true => {
            format!("{name} main is behind origin by {behind} and not fast-forwardable: {reason}")
        }
        false => format!("{name} main refused offline: {reason}"),
    })
}

fn reattach<T>(git: &impl Fn(&[&str]) -> Option<T>) -> Result<(), String> {
    let main_absent = git(&["rev-parse", "-q", "--verify", "refs/heads/main"]).is_none();
    if !main_absent && git(&["merge-base", "--is-ancestor", "refs/heads/main", "origin/main"]).is_none() {
        return Err("checkout is off main and local main is not an ancestor of origin/main".to_owned());
    }
    git(&["switch", "-q", "main"])
        .map(drop)
        .ok_or_else(|| "checkout is off main and git switch main failed".to_owned())
}

fn fast_forward<T>(directory: &Path, fetched: bool, git: &impl Fn(&[&str]) -> Option<T>) -> Option<String> {
    if !fetched || git(&["merge", "-q", "--ff-only", "origin/main"]).is_some() {
        return None;
    }
    let differing = clear_identical_untracked(directory);
    if !differing.is_empty() {
        return Some(format!("untracked files with different bytes block the fast-forward: {}", differing.join(", ")));
    }
    git(&["merge", "-q", "--ff-only", "origin/main"]).is_none().then(|| "git merge --ff-only failed".to_owned())
}

fn clear_identical_untracked(directory: &Path) -> Vec<String> {
    let lines = |args: &[&str]| -> Vec<String> {
        git::run(directory, args)
            .ok()
            .filter(|output| output.status.success())
            .map(|output| output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| String::from_utf8_lossy(path).into_owned()).collect())
            .unwrap_or_default()
    };
    let incoming = lines(&["diff", "-z", "--no-renames", "--name-only", "--diff-filter=A", "HEAD", "origin/main"]);
    let mut differing = Vec::new();
    for path in lines(&["ls-files", "-z", "--others", "--exclude-standard"]).into_iter().filter(|path| incoming.contains(path)) {
        let blob = git::run(directory, &["cat-file", "blob", &format!("origin/main:{path}")]).ok().filter(|output| output.status.success());
        let file = directory.join(&path);
        let identical = blob.is_some_and(|blob| std::fs::symlink_metadata(&file).is_ok_and(|meta| meta.is_file()) && std::fs::read(&file).is_ok_and(|bytes| bytes == blob.stdout));
        if !identical || std::fs::remove_file(&file).is_err() {
            differing.push(path);
        }
    }
    differing
}

pub(super) fn north_checkout(
    roots: Option<&str>,
    north_home: Option<std::ffi::OsString>,
    home: &Path,
) -> NorthResult<PathBuf> {
    let configured = match roots {
        Some(roots) => serde_json::from_str::<serde_json::Value>(roots)?
            .get("north")
            .and_then(|path| path.as_str().map(PathBuf::from)),
        None => None,
    };
    Ok(configured
        .or_else(|| {
            north_home
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home.join("code/north/main")))
}

fn shared_configuration(home: &Path, south: &Path, now: u64, say: &mut impl FnMut(String)) -> Result<(), String> {
    match shared_config::refresh(home, south, now) {
        Ok(stamp) => {
            say(format!("agents sync: shared configuration at south {}", &stamp.revision[..10]));
            Ok(())
        }
        Err(error) => {
            say(format!("agents sync: shared configuration fetch failed: {error}"));
            shared_config::fresh(home, now).map(drop)
        }
    }
}

const FETCH_STAMP: &str = "north-last-fetch";

fn fetch_stamp(directory: &Path) -> Option<PathBuf> {
    let output = git::run(directory, &["rev-parse", "--path-format=absolute", "--git-path", FETCH_STAMP]).ok()?;
    output.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

fn stamp_fetch(directory: &Path, now: u64) {
    if let Some(path) = fetch_stamp(directory) {
        let _ = std::fs::write(path, now.to_string());
    }
}

fn last_fetch(directory: &Path) -> Option<u64> {
    std::fs::read_to_string(fetch_stamp(directory)?).ok()?.trim().parse().ok()
}
