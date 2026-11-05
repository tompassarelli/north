use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::gate::Verdicts;

pub(super) struct HostClass {
    pub(super) name: String,
    pub(super) bins: Vec<PathBuf>,
}

struct Mention {
    program: String,
    bare: bool,
    file: PathBuf,
    line: usize,
}

static INVOCATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:[Rr]un|[Uu]se|[Ii]nvoke|[Cc]all|[Ee]xecute|[Ss]tart|[Ll]aunch)\s+`([a-z][a-z0-9_-]*)([^`]*)`").unwrap());

static PROHIBITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:never|not|avoid|without|don't)\s+$").unwrap());

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_-]*$").unwrap());

fn skill_files(skills: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(skills) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path().join("SKILL.md")).filter(|file| file.is_file()).collect();
    files.sort();
    files
}

fn skill_names(skills: &Path) -> Vec<String> {
    fs::read_dir(skills).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect()
}

fn mentions(file: &Path) -> Vec<Mention> {
    let text = fs::read_to_string(file).unwrap_or_default();
    let mut found = Vec::new();
    let mut fenced = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let programs: Vec<(String, bool)> = if fenced {
            line.split_whitespace().next().filter(|word| WORD.is_match(word)).map(|word| (word.to_owned(), false)).into_iter().collect()
        } else {
            INVOCATION.captures_iter(line).filter(|capture| !PROHIBITION.is_match(&line[..capture.get(0).map_or(0, |whole| whole.start())])).map(|capture| (capture[1].to_owned(), capture[2].trim().is_empty())).collect()
        };
        found.extend(programs.into_iter().map(|(program, bare)| Mention { program, bare, file: file.to_path_buf(), line: index + 1 }));
    }
    found
}

fn executable_in(directory: &Path, program: &str) -> bool {
    fs::metadata(directory.join(program)).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub(super) fn skill_programs(root: &Path, runtime_bin: &Path, hosts: &[HostClass]) -> Verdicts {
    let skills = root.join("harness/skills");
    let names = skill_names(&skills);
    let mut errors = Vec::new();
    let mut checked = 0;
    for mention in skill_files(&skills).iter().flat_map(|file| mentions(file)) {
        if mention.bare && names.contains(&mention.program) {
            continue;
        }
        checked += 1;
        let missing: Vec<&str> = hosts
            .iter()
            .filter(|host| !executable_in(runtime_bin, &mention.program) && !host.bins.iter().any(|directory| executable_in(directory, &mention.program)))
            .map(|host| host.name.as_str())
            .collect();
        if !missing.is_empty() {
            let place = mention.file.strip_prefix(root).unwrap_or(&mention.file).display();
            errors.push(format!(
                "{place}:{}: {} is on no PATH of {}; install it there, or name a program that is installed",
                mention.line,
                mention.program,
                missing.join(", ")
            ));
        }
    }
    let text = if errors.is_empty() {
        format!("{checked} programs that skills tell agents to run resolve on {}", hosts.iter().map(|host| host.name.as_str()).collect::<Vec<_>>().join(", "))
    } else {
        errors.join("\n")
    };
    Verdicts { text, passed: errors.is_empty() }
}

pub(super) fn local_host(runtime_bin: &Path) -> HostClass {
    let name = fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_else(|_| "this host".into());
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut bins: Vec<PathBuf> = std::env::split_paths(&path).collect();
    bins.push(runtime_bin.to_path_buf());
    HostClass { name, bins }
}
