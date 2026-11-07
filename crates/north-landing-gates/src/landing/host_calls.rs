use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::rust_calls;

const EXEC_KEYS: [&str; 7] = ["ExecStart=", "ExecStartPre=", "ExecStartPost=", "ExecStop=", "ExecStopPost=", "ExecReload=", "ExecCondition="];
const UNIT_SUFFIXES: [&str; 4] = [".service", ".timer", ".path", ".socket"];
const SCRIPT_BYTES: u64 = 1 << 20;
const SHELL_STOPS: [char; 11] = ['|', '&', ';', '>', '<', '$', '\'', '"', '(', ')', '`'];
const CLOSERS: [char; 5] = ['"', '\'', ';', ')', ','];
const SETTINGS: &str = ".claude/settings.json";
const SSH: &str = "ssh";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Unit,
    Hook,
    /// A call in a Rust source that also builds an ssh command, so another host's runtime may be the one that runs it.
    Hop,
    Rust,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCall {
    pub source: Source,
    pub location: String,
    pub words: Vec<String>,
}

#[derive(Clone, Copy)]
pub enum Scope {
    System,
    User,
}

impl Scope {
    fn flags(self) -> &'static [&'static str] {
        match self {
            Scope::System => &[],
            Scope::User => &["--user"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Scope::System => "system",
            Scope::User => "user",
        }
    }
}

fn separator(c: char) -> bool {
    c.is_ascii_whitespace() || "'\"(;|&`=".contains(c)
}

fn named_north(line: &str, at: usize) -> bool {
    let start = line[..at].rfind(separator).map_or(0, |before| before + 1);
    let program = &line[start..at];
    if !(program.is_empty() || program.ends_with('/') && program.starts_with(['/', '$', '~', '.'])) {
        return false;
    }
    !line[..start].split_whitespace().next_back().is_some_and(|before| before.starts_with('-') && before != "--")
}

pub fn invocations(line: &str) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    for (at, _) in line.match_indices("north ") {
        if !named_north(line, at) {
            continue;
        }
        let mut words: Vec<String> = Vec::new();
        for word in line[at + "north ".len()..].split_whitespace() {
            let bare = word.trim_end_matches(CLOSERS);
            if bare.is_empty() || bare.starts_with('-') || bare.contains(SHELL_STOPS) {
                break;
            }
            words.push(bare.to_owned());
            if bare.len() < word.len() {
                break;
            }
        }
        if words.first().is_some_and(|word| word.starts_with(|c: char| c.is_ascii_lowercase())) {
            found.push(words);
        }
    }
    found
}

fn exec_value(line: &str) -> Option<&str> {
    EXEC_KEYS.iter().find_map(|key| line.trim_start().strip_prefix(key))
}

fn store_programs(exec: &str) -> impl Iterator<Item = &str> {
    exec.split_whitespace().map(|word| word.trim_start_matches(['@', '-', ':', '+', '!'])).filter(|word| word.starts_with("/nix/store/"))
}

fn script_text(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(SCRIPT_BYTES).read_to_end(&mut bytes).ok()?;
    (!bytes.contains(&0)).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn unit_calls(listing: &str, script: impl Fn(&Path) -> Option<String>) -> Vec<HostCall> {
    let mut unit = String::new();
    let mut number = 0;
    let mut found = Vec::new();
    let mut scripts: Vec<(String, String)> = Vec::new();
    for line in listing.lines() {
        if let Some(path) = line.strip_prefix("# /") {
            unit = format!("/{}", path.trim());
            number = 0;
            continue;
        }
        number += 1;
        let Some(exec) = exec_value(line) else { continue };
        let location = format!("{unit}:{number}");
        found.extend(invocations(exec).into_iter().map(|words| HostCall { source: Source::Unit, location: location.clone(), words }));
        for program in store_programs(exec) {
            if !scripts.iter().any(|(seen, _)| seen == program) {
                scripts.push((program.to_owned(), location.clone()));
            }
        }
    }
    for (program, runner) in scripts {
        let Some(text) = script(Path::new(&program)) else { continue };
        for (index, line) in text.lines().enumerate().filter(|(_, line)| !line.trim_start().starts_with('#')) {
            let location = format!("{program}:{} (run by {runner})", index + 1);
            found.extend(invocations(line).into_iter().map(|words| HostCall { source: Source::Unit, location: location.clone(), words }));
        }
    }
    found
}

pub fn units(scope: Scope) -> Result<String, String> {
    let systemctl = |args: &[&str]| {
        Command::new("systemctl")
            .args(scope.flags())
            .args(args)
            .output()
            .map_err(|error| format!("systemctl: {error}"))
    };
    let listing = systemctl(&["list-unit-files", "--no-legend", "--no-pager"])?;
    if !listing.status.success() {
        return Err(format!("systemctl {} list-unit-files failed: {}", scope.label(), String::from_utf8_lossy(&listing.stderr).trim()));
    }
    let listing = String::from_utf8_lossy(&listing.stdout);
    let names: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| UNIT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) && !name.ends_with("@.service"))
        .collect();
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut arguments = vec!["cat", "--no-pager", "--"];
    arguments.extend(names);
    let shown = systemctl(&arguments)?;
    Ok(String::from_utf8_lossy(&shown.stdout).into_owned())
}

pub fn hook_calls(location: &str, settings: &str) -> Vec<HostCall> {
    settings
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("\"command\""))
        .flat_map(|(index, line)| invocations(line).into_iter().map(move |words| HostCall { source: Source::Hook, location: format!("{location}:{}", index + 1), words }))
        .collect()
}

fn rust_sources(directory: &Path, root: &Path, found: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect();
    paths.sort();
    for path in paths {
        let relative = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        if path.is_dir() {
            rust_sources(&path, root, found);
        } else if rust_calls::is_north_code(&relative) {
            found.push((relative, path));
        }
    }
}

pub fn tree_calls(tree: &Path) -> Vec<HostCall> {
    let mut sources = Vec::new();
    for root in rust_calls::SOURCES {
        rust_sources(&tree.join(root), tree, &mut sources);
    }
    sources
        .into_iter()
        .filter_map(|(relative, path)| Some((relative, fs::read_to_string(path).ok()?)))
        .flat_map(|(relative, text)| {
            let source = if text.contains(SSH) { Source::Hop } else { Source::Rust };
            rust_calls::launched(&text)
                .into_iter()
                .map(move |call| HostCall { source, location: format!("{relative}:{}", call.line), words: call.words })
        })
        .collect()
}

pub struct Gathered {
    pub calls: Vec<HostCall>,
    pub warnings: Vec<String>,
}

pub fn on_this_host(home: &Path, tree: Option<&Path>) -> Gathered {
    let mut calls = Vec::new();
    let mut warnings = Vec::new();
    for scope in [Scope::System, Scope::User] {
        match units(scope) {
            Ok(listing) => calls.extend(unit_calls(&listing, script_text)),
            Err(error) => warnings.push(format!("cannot read this host's {} units ({error}), so they were not checked", scope.label())),
        }
    }
    let settings = home.join(SETTINGS);
    if let Ok(text) = fs::read_to_string(&settings) {
        calls.extend(hook_calls(&settings.display().to_string(), &text));
    }
    match tree {
        Some(tree) => calls.extend(tree_calls(tree)),
        None => warnings.push("no runtime tree was given, so north's own calls to itself were not checked".to_owned()),
    }
    Gathered { calls, warnings }
}
