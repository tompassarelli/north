use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOTS: [&str; 5] = ["bin", "lib", "harness/hooks", "harness/lib", "harness/scripts"];
const MACHINE_BOUND: &str = "harness/machine-bound-tests";
pub const SELECT_PATHS_ENV: &str = "SAFE_PUSH_LAND_SELECT_PATHS";
const NORTH_BINARY: [&str; 3] = ["NORTH_PACE_BIN", "NORTH_BIN", "debug/north"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Changed,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Only {
    Both,
    Portable,
    MachineBound,
}

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub status: u8,
}

impl Failure {
    fn new(message: String) -> Self {
        Failure { message, status: 1 }
    }
}

#[derive(Default)]
pub struct ChildEnv {
    set: Vec<(&'static str, OsString)>,
    remove: Vec<&'static str>,
}

impl ChildEnv {
    pub fn for_selection(root: &Path) -> Self {
        let path = std::env::var_os("PATH").map(|path| pinned_path(&path)).unwrap_or_default();
        let mut set = vec![("PATH", path)];
        if root.join("Cargo.toml").is_file()
            && let Ok(north) = std::env::current_exe()
        {
            set.push(("NORTH_BIN", north.clone().into_os_string()));
            set.push(("NORTH_PACE_BIN", north.into_os_string()));
        }
        ChildEnv { set, remove: vec!["THREADS_ACTOR"] }
    }

    pub fn apply<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        for name in &self.remove {
            command.env_remove(name);
        }
        for (name, value) in &self.set {
            command.env(name, value);
        }
        command
    }
}

// A home or system switch replaces the profile symlinks on PATH mid-run; pin every directory to its immutable target.
fn pinned_path(path: &std::ffi::OsStr) -> OsString {
    let pinned: Vec<PathBuf> = std::env::split_paths(path).map(|dir| fs::canonicalize(&dir).unwrap_or(dir)).collect();
    std::env::join_paths(pinned).unwrap_or_else(|_| path.to_owned())
}

pub fn select(root: &Path, scope: Scope, only: Only, env: &ChildEnv) -> Result<Vec<String>, Failure> {
    let machine_bound = machine_bound(root)?;
    let changes = match scope {
        Scope::All => Vec::new(),
        Scope::Changed => changes(root, env)?,
    };
    let mut walk = Walk { root, only, machine_bound, seen: HashSet::new(), selected: Vec::new(), visited: HashSet::new(), queue: VecDeque::new() };
    let roots: Vec<&str> = ROOTS.into_iter().filter(|dir| root.join(dir).is_dir()).collect();
    let mut files = Vec::new();
    for dir in roots {
        files_under(root, dir, &mut files)?;
    }
    if root.join("harness").is_dir() {
        files.extend(listing(root, "harness")?.into_iter().filter(|(path, kind)| kind.is_file() && path.ends_with(".toml")).map(|(path, _)| path));
    }
    for path in &changes {
        if ["bin/", "lib/", "harness/hooks/", "harness/lib/", "harness/scripts/"].iter().any(|prefix| path.starts_with(prefix))
            || (path.starts_with("harness/") && path.ends_with(".toml"))
        {
            walk.enqueue(path);
        } else if path.starts_with("src/") || path.starts_with("crates/") || ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"].contains(&path.as_str()) {
            walk.enqueue(path);
            NORTH_BINARY.into_iter().for_each(|name| walk.enqueue(name));
        }
    }
    if scope == Scope::All {
        files.iter().filter(|file| is_test(file)).for_each(|file| walk.add(file));
    }
    while let Some(path) = walk.queue.pop_front() {
        let stem = test_stem(&path);
        walk.add(&format!("{stem}.test.sh"));
        walk.add(&format!("{stem}.test.py"));
        if files.is_empty() {
            continue;
        }
        let matches = referencing(root, &patterns(&path), &files)?;
        for file in matches {
            if is_test(&file) {
                walk.add(&file);
            }
            walk.enqueue(&file);
        }
    }
    Ok(walk.selected)
}

struct Walk<'a> {
    root: &'a Path,
    only: Only,
    machine_bound: HashSet<String>,
    seen: HashSet<String>,
    selected: Vec<String>,
    visited: HashSet<String>,
    queue: VecDeque<String>,
}

impl Walk<'_> {
    fn add(&mut self, test: &str) {
        if !self.root.join(test).is_file() || self.seen.contains(test) {
            return;
        }
        let bound = self.machine_bound.contains(test);
        if (self.only == Only::Portable && bound) || (self.only == Only::MachineBound && !bound) {
            return;
        }
        self.seen.insert(test.to_owned());
        self.selected.push(test.to_owned());
    }

    fn enqueue(&mut self, path: &str) {
        if self.visited.insert(path.to_owned()) {
            self.queue.push_back(path.to_owned());
        }
    }

}

fn listing(root: &Path, dir: &str) -> Result<Vec<(String, fs::FileType)>, Failure> {
    let entries = fs::read_dir(root.join(dir)).map_err(|error| Failure::new(format!("land check: cannot list {dir}: {error}")))?;
    let mut listed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Failure::new(format!("land check: cannot list {dir}: {error}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type().map_err(|error| Failure::new(format!("land check: cannot read {dir}/{name}: {error}")))?;
        listed.push((format!("{dir}/{name}"), kind));
    }
    listed.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(listed)
}

fn files_under(root: &Path, dir: &str, files: &mut Vec<String>) -> Result<(), Failure> {
    for (path, kind) in listing(root, dir)? {
        if kind.is_dir() {
            files_under(root, &path, files)?;
        } else if kind.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn referencing(root: &Path, patterns: &[String], files: &[String]) -> Result<Vec<String>, Failure> {
    let alternatives: Vec<String> = patterns.iter().map(|pattern| format!("(?:{pattern})")).collect();
    let pattern = regex::bytes::Regex::new(&alternatives.join("|")).map_err(|error| Failure::new(format!("land check: bad reference pattern: {error}")))?;
    let mut matches = Vec::new();
    for file in files {
        let bytes = fs::read(root.join(file)).map_err(|error| Failure::new(format!("land check: cannot read {file}: {error}")))?;
        if !bytes.contains(&0) && bytes.split(|byte| *byte == b'\n').any(|line| pattern.is_match(line)) {
            matches.push(file.clone());
        }
    }
    Ok(matches)
}

fn machine_bound(root: &Path) -> Result<HashSet<String>, Failure> {
    let Ok(listed) = fs::read_to_string(root.join(MACHINE_BOUND)) else {
        return Ok(HashSet::new());
    };
    let tests: HashSet<String> = listed.lines().filter(|line| !line.starts_with('#') && !line.trim().is_empty()).map(str::to_owned).collect();
    let mut missing: Vec<&str> = tests.iter().filter(|test| !root.join(test).is_file()).map(String::as_str).collect();
    missing.sort_unstable();
    if missing.is_empty() {
        Ok(tests)
    } else {
        Err(Failure::new(format!("land check: {MACHINE_BOUND} lists files that do not exist: {}", missing.join(" "))))
    }
}

fn changes(root: &Path, env: &ChildEnv) -> Result<Vec<String>, Failure> {
    if let Ok(paths) = std::env::var(SELECT_PATHS_ENV) {
        return Ok(paths.lines().filter(|path| !path.is_empty()).map(str::to_owned).collect());
    }
    let revision = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let (Some(base), Some(head)) = (revision("SAFE_PUSH_LAND_BASE"), revision("SAFE_PUSH_LAND_HEAD")) else {
        return Err(Failure::new("land check: --select changed needs SAFE_PUSH_LAND_BASE and SAFE_PUSH_LAND_HEAD".to_owned()));
    };
    let output = env.apply(Command::new("git").current_dir(root).args(["diff", "--name-only", "--no-renames", "-z", &base, &head])).output();
    match output {
        Ok(output) if output.status.success() => Ok(output.stdout.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()).map(|entry| String::from_utf8_lossy(entry).into_owned()).collect()),
        _ => Err(Failure::new("land check: git diff failed".to_owned())),
    }
}

fn is_test(path: &str) -> bool {
    path.ends_with(".test.sh") || path.ends_with(".test.py")
}

fn test_stem(path: &str) -> &str {
    let mut stem = path;
    for suffix in [".sh", ".py", ".ts", ".mjs"] {
        stem = stem.strip_suffix(suffix).unwrap_or(stem);
    }
    stem.strip_suffix(".test").unwrap_or(stem)
}

fn patterns(path: &str) -> Vec<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    let name = match path.rsplit_once('/') {
        Some((dir, _)) if !base.contains('.') => format!("{}/{base}", dir.rsplit('/').next().unwrap_or(dir)),
        _ => base.to_owned(),
    };
    let mut patterns = vec![format!("(^|[^[:alnum:]_.-]){}([^[:alnum:]_.-]|$)", regex::escape(&name))];
    if let Some(module) = base.strip_suffix(".py").filter(|module| python_identifier(module)) {
        patterns.push(format!(
            r"(^|[;:])[[:space:]]*(from[[:space:]]+{module}[[:space:]]+import\b|import[[:space:]]+([[:alnum:]_.]+([[:space:]]+as[[:space:]]+[[:alnum:]_]+)?[[:space:]]*,[[:space:]]*)*{module}([[:space:]]*(,|;|#|$)|[[:space:]]+as[[:space:]]))"
        ));
    }
    patterns
}

fn python_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_') && characters.all(|rest| rest.is_ascii_alphanumeric() || rest == '_')
}
