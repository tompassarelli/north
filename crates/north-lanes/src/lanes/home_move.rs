//! `north home move PLAN.tsv`: apply a move plan at the top of the home directory from outside the
//! worker sandbox.
//!
//! A worker cannot rename entries at the top of home from inside its sandbox, so such a rename
//! fails with EACCES (north#992). This verb runs
//! through the outside broker and applies a plan under its own rule: one rename per row, never a
//! copy or a delete, within one filesystem, and nothing that a config, a user unit or a live process
//! names. It appends each completed row to `moves.tsv` beside the plan before it reports the move.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{Mode, OFlags, RenameFlags, ResolveFlags};
use rustix::io::Errno;

mod references;

use self::references::References;

const USAGE: &str = "usage: north home move [--dry-run] PLAN.tsv

Applies each FROM<TAB>TO row of PLAN.tsv, written relative to $HOME or as an absolute path, as one
rename of an entry directly under the top of $HOME. It runs outside a worker sandbox, where the
kernel lets a worker remove only inside its lane. A move never copies and never deletes: the
destination must not exist, both paths must be on one filesystem, and a row is refused when either
path names a dotfile or dot-directory, a git checkout, the ~/code container, a destination
reached through a symbolic link, or a path that a config, a user unit or a live process names.
Each completed row is appended to moves.tsv beside the plan before it is reported. --dry-run checks
every row and prints what would move without moving or logging anything.";

const LOG: &str = "moves.tsv";
const SYMLINK: &str = "the destination passes through a symbolic link";

pub fn run(arguments: &[String]) -> u8 {
    let (plan, dry_run) = match options(arguments) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let Some(home) = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        eprintln!("home move: HOME is not set");
        return 1;
    };
    let (done, failed) = apply(&plan, &home, dry_run, &References::load(&home));
    for line in &done {
        println!("{line}");
    }
    for line in &failed {
        eprintln!("{line}");
    }
    if failed.is_empty() { 0 } else { 1 }
}

fn options(arguments: &[String]) -> Result<(PathBuf, bool), u8> {
    if arguments
        .iter()
        .any(|argument| argument == "-h" || argument == "--help")
    {
        println!("{USAGE}");
        return Err(0);
    }
    let dry_run = arguments.iter().any(|argument| argument == "--dry-run");
    let plans: Vec<&String> = arguments
        .iter()
        .filter(|argument| *argument != "--dry-run")
        .collect();
    match plans.as_slice() {
        [plan] if !plan.starts_with('-') => Ok((PathBuf::from(plan), dry_run)),
        _ => {
            eprintln!("{USAGE}");
            Err(2)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Row {
    from: String,
    to: String,
}

fn read_plan(plan: &Path) -> Result<Vec<Row>, String> {
    let text = fs::read_to_string(plan)
        .map_err(|error| format!("cannot read {}: {error}", plan.display()))?;
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty()
            || line.trim_start().starts_with('#')
            || line.trim_end() == "from\tto"
        {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 2 || fields.iter().any(|field| field.trim().is_empty()) {
            return Err(format!(
                "{}:{}: each row is FROM<TAB>TO, with no third field",
                plan.display(),
                number + 1
            ));
        }
        rows.push(Row {
            from: fields[0].trim().to_owned(),
            to: fields[1].trim().to_owned(),
        });
    }
    if rows.is_empty() {
        return Err(format!(
            "{}: the plan holds no FROM<TAB>TO row",
            plan.display()
        ));
    }
    Ok(rows)
}

/// Applies every row of `plan` under `home`, returning the lines to print as done and as kept. A row
/// is independent: one refusal does not stop the rest, and each successful move is logged before its
/// line is returned.
pub(super) fn apply(
    plan: &Path,
    home: &Path,
    dry_run: bool,
    references: &References,
) -> (Vec<String>, Vec<String>) {
    let rows = match read_plan(plan) {
        Ok(rows) => rows,
        Err(reason) => return (Vec::new(), vec![reason]),
    };
    let plan = match absolute(plan) {
        Ok(plan) => plan,
        Err(reason) => return (Vec::new(), vec![reason]),
    };
    if !home.is_absolute() {
        return (
            Vec::new(),
            vec![format!(
                "the home directory {} is not absolute",
                home.display()
            )],
        );
    }
    let home = home.to_path_buf();
    let log_path = plan.with_file_name(LOG);
    let (steps, failed) = steps(&home, &plan, &log_path, references, rows);
    if dry_run {
        return (
            steps
                .into_iter()
                .map(|(row, _, _)| format!("would move: {} -> {}", row.from, row.to))
                .collect(),
            failed,
        );
    }
    run_steps(&home, steps, &log_path, failed)
}

fn run_steps(
    home: &Path,
    steps: Vec<(Row, PathBuf, PathBuf)>,
    log_path: &Path,
    mut failed: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    if steps.is_empty() {
        return (Vec::new(), failed);
    }
    let mut log = match open_log(log_path) {
        Ok(file) => file,
        Err(reason) => {
            failed.push(reason);
            return (Vec::new(), failed);
        }
    };
    let mut done = Vec::new();
    for (row, from, to) in steps {
        if let Err(reason) = rename(home, &from, &to) {
            failed.push(format!("kept {}: {reason}", row.from));
            continue;
        }
        if let Err(reason) = log_move(&mut log, &row) {
            failed.push(format!(
                "moved {} -> {} but could not log it: {reason}",
                row.from, row.to
            ));
            break;
        }
        done.push(format!("moved: {} -> {}", row.from, row.to));
    }
    (done, failed)
}

fn steps(
    home: &Path,
    plan: &Path,
    log: &Path,
    references: &References,
    rows: Vec<Row>,
) -> (Vec<(Row, PathBuf, PathBuf)>, Vec<String>) {
    let mut steps = Vec::new();
    let mut failed = Vec::new();
    for row in rows {
        match resolve(home, &row) {
            Ok((from, to)) => match validate(home, &from, &to, plan, log, references) {
                Ok(()) => steps.push((row, from, to)),
                Err(reason) => failed.push(format!("kept {}: {reason}", row.from)),
            },
            Err(reason) => failed.push(format!("kept {}: {reason}", row.from)),
        }
    }
    (steps, failed)
}

fn open_log(log: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|error| format!("cannot open {}: {error}", log.display()))
}

fn log_move(log: &mut File, row: &Row) -> Result<(), String> {
    writeln!(log, "{}\t{}", row.from, row.to)
        .and_then(|()| log.flush())
        .map_err(|error| error.to_string())
}

fn resolve(home: &Path, row: &Row) -> Result<(PathBuf, PathBuf), String> {
    Ok((path(home, &row.from)?, path(home, &row.to)?))
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot read the working directory: {error}"))?
            .join(path)
    };
    Ok(path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect())
}

fn path(home: &Path, field: &str) -> Result<PathBuf, String> {
    let raw = expand(home, field)?;
    if raw
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("{field} climbs out with .."));
    }
    Ok(raw
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect())
}

fn expand(home: &Path, field: &str) -> Result<PathBuf, String> {
    if field == "~" {
        return Ok(home.to_path_buf());
    }
    for prefix in ["~/", "${HOME}/", "$HOME/"] {
        if let Some(rest) = field.strip_prefix(prefix) {
            return Ok(home.join(rest));
        }
    }
    if field.starts_with('~') || field.contains("$HOME") {
        return Err(format!("{field} is not a path under $HOME"));
    }
    Ok(home.join(field))
}

fn validate(
    home: &Path,
    from: &Path,
    to: &Path,
    plan: &Path,
    log: &Path,
    references: &References,
) -> Result<(), String> {
    validate_paths(home, from, to, plan, log)?;
    validate_references(home, from, to, references)?;
    validate_move(home, from, to)
}

fn validate_paths(
    home: &Path,
    from: &Path,
    to: &Path,
    plan: &Path,
    log: &Path,
) -> Result<(), String> {
    validate_bounds(home, from, to, plan, log)?;
    validate_git(home, from, to)
}

fn validate_bounds(
    home: &Path,
    from: &Path,
    to: &Path,
    plan: &Path,
    log: &Path,
) -> Result<(), String> {
    validate_shape(home, from, to)?;
    validate_scope(home, from, to, plan, log)
}

fn validate_shape(home: &Path, from: &Path, to: &Path) -> Result<(), String> {
    if from == home || from.parent() != Some(home) {
        return Err("the source is not an entry directly under $HOME".to_owned());
    }
    if to == home || !to.starts_with(home) {
        return Err("the destination is not under $HOME".to_owned());
    }
    if dotted(home, from) || dotted(home, to) {
        return Err("a dotfile or dot-directory is outside a move plan".to_owned());
    }
    Ok(())
}

fn validate_scope(
    home: &Path,
    from: &Path,
    to: &Path,
    plan: &Path,
    log: &Path,
) -> Result<(), String> {
    if in_code(home, from) || in_code(home, to) {
        return Err("~/code is a container of checkouts".to_owned());
    }
    if plan.starts_with(from) || log.starts_with(from) {
        return Err("the plan or its log lives in the source".to_owned());
    }
    if to.starts_with(from) {
        return Err("the destination lies inside the source".to_owned());
    }
    Ok(())
}

fn validate_git(home: &Path, from: &Path, to: &Path) -> Result<(), String> {
    if git_checkout(from) || inside_checkout(home, to) {
        return Err("a git checkout is outside a move plan".to_owned());
    }
    Ok(())
}

fn validate_references(
    home: &Path,
    from: &Path,
    to: &Path,
    references: &References,
) -> Result<(), String> {
    if references.names(from, home) {
        return Err("a config, a user unit or a live process names it".to_owned());
    }
    if references.names(to, home) {
        return Err("a config, a user unit or a live process names the destination".to_owned());
    }
    Ok(())
}

fn validate_move(home: &Path, from: &Path, to: &Path) -> Result<(), String> {
    let source = fs::symlink_metadata(from).map_err(|error| format!("cannot read it: {error}"))?;
    let Some(parent) = to.parent() else {
        return Err("the destination has no parent".to_owned());
    };
    let directory = fs::metadata(parent)
        .map_err(|error| format!("the destination directory does not exist: {error}"))?;
    if !directory.is_dir() {
        return Err("the destination's parent is not a directory".to_owned());
    }
    if parent
        .ancestors()
        .take_while(|ancestor| *ancestor != home)
        .any(|ancestor| fs::symlink_metadata(ancestor).is_ok_and(|link| link.file_type().is_symlink()))
    {
        return Err(SYMLINK.to_owned());
    }
    if source.dev() != directory.dev() {
        return Err("the source and destination are on different filesystems".to_owned());
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err("the destination already exists".to_owned());
    }
    Ok(())
}

fn dotted(home: &Path, path: &Path) -> bool {
    path.strip_prefix(home)
        .into_iter()
        .flat_map(Path::components)
        .any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.starts_with('.'))
        })
}

fn in_code(home: &Path, path: &Path) -> bool {
    let code = home.join("code");
    path == code || path.starts_with(code)
}

fn git_checkout(path: &Path) -> bool {
    path.is_dir() && path.join(".git").symlink_metadata().is_ok()
}

fn inside_checkout(home: &Path, path: &Path) -> bool {
    path.ancestors()
        .take_while(|ancestor| *ancestor != home)
        .any(git_checkout)
}

fn rename(home: &Path, from: &Path, to: &Path) -> Result<(), String> {
    let (Some(name), Some(target), Some(below)) = (from.file_name(), to.file_name(), to.parent().and_then(|parent| parent.strip_prefix(home).ok())) else {
        return Err("the move is not beneath $HOME".to_owned());
    };
    let below = if below.as_os_str().is_empty() { Path::new(".") } else { below };
    let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let top = rustix::fs::open(home, flags, Mode::empty()).map_err(|error| format!("cannot open $HOME: {error}"))?;
    let directory = rustix::fs::openat2(&top, below, flags, Mode::empty(), ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS).map_err(|error| match error {
        Errno::LOOP | Errno::XDEV => SYMLINK.to_owned(),
        _ => format!("cannot open the destination directory: {error}"),
    })?;
    rustix::fs::renameat_with(&top, name, &directory, target, RenameFlags::NOREPLACE).map_err(|error| match error.kind() {
        std::io::ErrorKind::CrossesDevices => "the source and destination are on different filesystems (nothing was copied)".to_owned(),
        std::io::ErrorKind::AlreadyExists => "the destination already exists (nothing was replaced)".to_owned(),
        std::io::ErrorKind::PermissionDenied => "permission denied; the move runs outside the worker sandbox, so check the entry's owner and mode".to_owned(),
        _ => format!("rename failed: {error}"),
    })
}

#[cfg(test)]
#[path = "home_move_tests.rs"]
mod tests;
