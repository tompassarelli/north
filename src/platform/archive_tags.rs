use super::process;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

const GIT_LIMIT: Duration = Duration::from_secs(15);
const TAGS: &str = "refs/tags/archive/";

static REFS_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bRefs\s*:?\s+(.+)").unwrap());

pub(crate) fn repo_alias(name: &str) -> &str {
    if name == "nixos-config" { "firn" } else { name }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '/')
}

fn reference_at(text: &str, start: usize) -> Option<(&str, u64)> {
    let rest = &text[start..];
    let rest = rest.strip_prefix("tompassarelli/").unwrap_or(rest);
    let name_len = rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))?;
    if name_len == 0 || !rest[name_len..].starts_with('#') {
        return None;
    }
    let digits = &rest[name_len + 1..];
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    Some((&rest[..name_len], digits[..end].parse().ok()?))
}

fn references(text: &str) -> Vec<(&str, u64)> {
    let mut found = Vec::new();
    let mut previous: Option<char> = None;
    for (index, c) in text.char_indices() {
        if previous.is_none_or(|p| !is_name_char(p))
            && let Some(reference) = reference_at(text, index)
        {
            found.push(reference);
        }
        previous = Some(c);
    }
    found
}

pub(crate) fn mentions(message: &str, repo: &str, number: u64) -> bool {
    message.lines().any(|line| {
        REFS_LINE.captures(line).is_some_and(|found| {
            references(found.get(1).map_or("", |m| m.as_str()))
                .iter()
                .any(|(name, n)| repo_alias(name) == repo && *n == number)
        })
    })
}

fn git(path: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C"])
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    let output = process::run_command_within(command, None, GIT_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn checkout(repo: &str, cwd: Option<&Path>) -> PathBuf {
    let name = if repo == "firn" { "nixos-config" } else { repo };
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let container = home.join("code").join(name);
    let current = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .and_then(|dir| dir.canonicalize().ok());
    let root = container.canonicalize().unwrap_or_else(|_| container.clone());
    match current {
        Some(dir) if dir.starts_with(&root) => dir,
        _ => container.join("main"),
    }
}

fn remote_tags(path: &Path) -> Option<BTreeMap<String, String>> {
    let listing = git(path, &["ls-remote", "--tags", "origin", "refs/tags/archive/*"])?;
    let mut tags = BTreeMap::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let (Some(sha), Some(name), None) = (fields.next(), fields.next(), fields.next()) else {
            return None;
        };
        if !name.ends_with("^{}") {
            tags.insert(name.to_owned(), sha.to_owned());
        }
    }
    Some(tags)
}

fn local_tags(path: &Path) -> Option<BTreeMap<String, String>> {
    let listed = git(path, &["for-each-ref", "--format=%(refname) %(objectname)", TAGS])?;
    Some(
        listed
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(name, sha)| (name.to_owned(), sha.to_owned()))
            .collect(),
    )
}

fn fetch_missing(path: &Path, tags: &BTreeMap<String, String>) -> bool {
    let local = local_tags(path);
    let missing: Vec<String> = tags
        .iter()
        .filter(|(name, sha)| {
            let held = match &local {
                Some(local) => local.get(*name).cloned(),
                None => git(path, &["rev-parse", "--verify", name]).map(|out| out.trim().to_owned()),
            };
            held.as_deref().unwrap_or("") != sha.as_str()
        })
        .map(|(name, _)| format!("{name}:{name}"))
        .collect();
    if missing.is_empty() {
        return true;
    }
    let mut args = vec!["fetch", "--no-tags", "--no-write-fetch-head", "origin"];
    args.extend(missing.iter().map(String::as_str));
    git(path, &args).is_some()
}

fn named(tags: &BTreeMap<String, String>, keep: impl Fn(&str) -> bool) -> Vec<String> {
    tags.iter()
        .filter(|(_, sha)| keep(sha))
        .map(|(name, _)| name.strip_prefix("refs/tags/").unwrap_or(name).to_owned())
        .collect()
}

fn holders_of_mentions(path: &Path, tags: &BTreeMap<String, String>, repo: &str, number: u64) -> Option<Vec<String>> {
    let mut shas: Vec<&str> = tags.values().map(String::as_str).collect();
    shas.sort_unstable();
    shas.dedup();
    let log = if shas.is_empty() {
        String::new()
    } else {
        let mut args = vec!["log", "--format=%H%x00%B%x1e"];
        args.extend(&shas);
        args.push("--");
        git(path, &args)?
    };
    let hits: Vec<String> = log
        .split('\x1e')
        .filter_map(|entry| {
            let (_, message) = entry.split_once('\0')?;
            mentions(message, repo, number)
                .then(|| entry.trim_start_matches('\n').split('\0').next().unwrap_or("").to_owned())
        })
        .collect();
    if hits.is_empty() {
        return Some(Vec::new());
    }
    let contains: Vec<String> = hits.iter().map(|hit| format!("--contains={hit}")).collect();
    let mut args = vec!["for-each-ref", "--format=%(objectname)"];
    args.extend(contains.iter().map(String::as_str));
    args.push(TAGS);
    let holders = git(path, &args)?;
    let held: Vec<&str> = holders.split_whitespace().collect();
    Some(named(tags, |sha| held.contains(&sha)))
}

pub(crate) fn archives(repo: &str, number: u64, cwd: Option<&Path>) -> Vec<String> {
    let path = checkout(repo, cwd);
    if !path.is_dir() {
        return Vec::new();
    }
    let Some(tags) = remote_tags(&path) else {
        return Vec::new();
    };
    if !fetch_missing(&path, &tags) {
        return Vec::new();
    }
    holders_of_mentions(&path, &tags, repo, number).unwrap_or_else(|| {
        named(&tags, |sha| {
            mentions(&git(&path, &["log", "--format=%B", sha, "--"]).unwrap_or_default(), repo, number)
        })
    })
}

pub(crate) fn shell_quote(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c)) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

pub(crate) fn report(repo: &str, number: u64, cwd: Option<&Path>) -> String {
    archives(repo, number, cwd)
        .iter()
        .map(|tag| {
            let branch = tag.strip_prefix("archive/").unwrap_or(tag);
            let restore = shell_quote(&format!("{tag}:refs/heads/{branch}"));
            format!("  archived: {tag}\n    restore: git push origin {restore}\n")
        })
        .collect()
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let [repo, number] = arguments else {
        eprintln!("usage: north archives REPO NUMBER");
        return 2;
    };
    let Ok(number) = number.parse() else {
        eprintln!("north archives: NUMBER must be an issue number");
        return 2;
    };
    print!("{}", report(repo_alias(repo), number, None));
    0
}
