use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::bytes::Regex;

use crate::platform::git;

const ALLOWLIST: &str = "config/hardcoded-repo-paths.tsv";
const ALLOW_MARKER: &[u8] = b"hardcoded-repo-path:allow";
const MINIMUM_FILES: usize = 100;
const HEADER: &str = "# Declared hard-coded repository paths: <repo-relative path><TAB><reference count>.
# An entry is live only while the path still carries exactly that many
# references. A stale entry and a changed count are both hard failures, so
# this file is the entire inventory — there is no separate total to drift.
# `hardcoded-repo-path-check --write-allow` re-counts and prunes listed paths.
# It never adds a path: a brand-new coupling is declared by hand, on purpose.
";

static R1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{?HOME\}?/code/(nixos-config|north)\b").unwrap());
static R2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/home/[a-z][a-z0-9_-]*/code/").unwrap());
static R3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~/code/(nixos-config|north)/").unwrap());
static SKIPPED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(^|/)(out|node_modules|\.direnv|docs/private)(/|$)|(^|/)result[^/]*(/|$)").unwrap()
});

pub(super) struct Finding {
    pub(super) path: String,
    pub(super) rule: &'static str,
    pub(super) line: usize,
    pub(super) body: Vec<u8>,
}

pub(super) struct Failure(pub(super) String);

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument != "--write-allow") {
        eprintln!("usage: north hardcoded-repo-path-check [--write-allow]");
        return 2;
    }
    let write_allow = !arguments.is_empty();
    let environment = Environment::from_process();
    match check(&environment, write_allow) {
        Ok(code) => code,
        Err(Failure(message)) => {
            eprintln!("hardcoded-repo-path-check: {message}");
            2
        }
    }
}

pub(super) struct Environment {
    pub(super) cwd: PathBuf,
    pub(super) home: PathBuf,
    pub(super) root_override: Option<PathBuf>,
    pub(super) roots_override: Option<String>,
}

impl Environment {
    fn from_process() -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Self {
            cwd: logical_cwd(),
            home: set("HOME").map(PathBuf::from).unwrap_or_default(),
            root_override: set("HARDCODED_REPO_PATH_CHECK_ROOT").map(PathBuf::from),
            roots_override: set("HARDCODED_REPO_PATH_CHECK_ROOTS").map(|value| value.to_string_lossy().into_owned()),
        }
    }

    fn source_root(&self) -> Option<PathBuf> {
        self.root_override
            .clone()
            .or_else(|| self.cwd.ancestors().find(|dir| dir.join(ALLOWLIST).is_file()).map(Path::to_path_buf))
    }

    fn roots(&self, source_root: Option<&Path>) -> Vec<PathBuf> {
        match &self.roots_override {
            Some(roots) => roots.split(':').filter(|root| !root.is_empty()).map(PathBuf::from).collect(),
            None => source_root
                .map(Path::to_path_buf)
                .into_iter()
                .chain([self.home.join("code/north/main")])
                .collect(),
        }
    }
}

pub(super) fn check(environment: &Environment, write_allow: bool) -> Result<u8, Failure> {
    let source_root = environment.source_root();
    let findings = scan_roots(&environment.roots(source_root.as_deref()))?;
    let allowlist = source_root
        .map(|root| root.join(ALLOWLIST))
        .filter(|path| path.is_file())
        .ok_or_else(|| Failure(format!("missing allowlist: {}", environment.root_override.as_ref().unwrap_or(&environment.cwd).join(ALLOWLIST).display())))?;
    let mut declared = parse_allowlist(&allowlist)?;
    let (allowed, new, mut found) = tally(&findings, &declared);
    if write_allow {
        rewrite_allowlist(&allowlist, &declared, &found)?;
        declared.retain(|path, _| found.get(path).copied().unwrap_or(0) > 0);
        for (path, want) in &mut declared {
            *want = found[path];
        }
    }
    found.retain(|path, _| declared.contains_key(path));
    let stale = report_stale(&declared, &found);
    println!("hardcoded-repo-path-check: {allowed} allowed, {new} new");
    Ok(if stale { 2 } else { u8::from(new > 0) })
}

fn scan_roots(roots: &[PathBuf]) -> Result<Vec<Finding>, Failure> {
    let mut findings = Vec::new();
    let mut file_count = 0;
    for root in roots {
        let inside = root.is_dir()
            && git::run_without_local_env(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|output| output.status.success());
        if !inside {
            return Err(Failure(format!("root is not a Git checkout: {}", root.display())));
        }
        let repository = repository_name(root);
        for path in tracked_files(root)? {
            let full = root.join(&path);
            if is_candidate(&path, &full) {
                file_count += 1;
                findings.extend(scan_file(&format!("{repository}/{path}"), &full));
            }
        }
    }
    if file_count < MINIMUM_FILES {
        return Err(Failure(format!("scanned only {file_count} files (minimum {MINIMUM_FILES})")));
    }
    Ok(findings)
}

fn repository_name(root: &Path) -> String {
    let mut container = root.parent().unwrap_or(root);
    if matches!(container.file_name().and_then(|name| name.to_str()), Some("worktrees" | "pins")) {
        container = container.parent().unwrap_or(container);
    }
    container.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn tracked_files(root: &Path) -> Result<Vec<String>, Failure> {
    let output = git::run_without_local_env(root, &["ls-files", "-z"])
        .map_err(|error| Failure(error.to_string()))?;
    if !output.status.success() {
        return Err(Failure(format!("git ls-files failed in {}", root.display())));
    }
    Ok(output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| String::from_utf8_lossy(path).into_owned()).collect())
}

fn is_candidate(path: &str, full: &Path) -> bool {
    let flake_lock = path == "flake.lock" || path.ends_with("/flake.lock");
    let compiled_nix = path.ends_with(".nix") && full.with_extension("bnix").is_file();
    !flake_lock && !SKIPPED.is_match(path) && full.is_file() && !compiled_nix
}

fn scan_file(logical: &str, full: &Path) -> Vec<Finding> {
    let Ok(content) = fs::read(full) else {
        return Vec::new();
    };
    if content.contains(&0) {
        return Vec::new();
    }
    let executable = fs::metadata(full).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0);
    let mut rules = vec![("R1", &*R1), ("R2", &*R2)];
    if executable {
        rules.push(("R3", &*R3));
    }
    let lines = content.split(|byte| *byte == b'\n').enumerate().collect::<Vec<_>>();
    rules
        .into_iter()
        .flat_map(|(rule, pattern)| {
            lines
                .iter()
                .filter(|(_, line)| pattern.is_match(line) && !line.windows(ALLOW_MARKER.len()).any(|window| window == ALLOW_MARKER))
                .map(move |(index, line)| Finding {
                    path: logical.to_string(),
                    rule,
                    line: index + 1,
                    body: line.to_vec(),
                })
        })
        .collect()
}

fn parse_allowlist(path: &Path) -> Result<BTreeMap<String, usize>, Failure> {
    let text = fs::read(path).map_err(|error| Failure(error.to_string()))?;
    let mut declared = BTreeMap::new();
    for (index, entry) in String::from_utf8_lossy(&text).split('\n').map(|entry| entry.trim_matches([' ', '\t'])).enumerate() {
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        let count = entry
            .split_once('\t')
            .filter(|(_, count)| !count.starts_with('0') && !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|(_, count)| count.parse::<usize>().ok());
        let (Some((declared_path, _)), Some(count)) = (entry.split_once('\t'), count) else {
            return Err(Failure(format!("malformed allowlist entry at {}:{}: {entry}", path.display(), index + 1)));
        };
        if declared.insert(declared_path.to_string(), count).is_some() {
            return Err(Failure(format!("duplicate allowlist entry: {declared_path}")));
        }
    }
    Ok(declared)
}

fn tally(findings: &[Finding], declared: &BTreeMap<String, usize>) -> (usize, usize, BTreeMap<String, usize>) {
    let (mut allowed, mut new) = (0, 0);
    let mut found = BTreeMap::new();
    for finding in findings {
        if declared.contains_key(&finding.path) {
            allowed += 1;
            *found.entry(finding.path.clone()).or_insert(0) += 1;
        } else {
            new += 1;
            let mut line = format!("WARN {}:{} [{}] ", finding.path, finding.line, finding.rule).into_bytes();
            line.extend_from_slice(&finding.body);
            line.push(b'\n');
            let _ = std::io::stderr().write_all(&line);
        }
    }
    (allowed, new, found)
}

fn rewrite_allowlist(path: &Path, declared: &BTreeMap<String, usize>, found: &BTreeMap<String, usize>) -> Result<(), Failure> {
    let mut entries = declared
        .keys()
        .filter_map(|declared_path| found.get(declared_path).filter(|count| **count > 0).map(|count| format!("{declared_path}\t{count}\n")))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    fs::write(path, format!("{HEADER}{}", entries.concat())).map_err(|error| Failure(error.to_string()))?;
    eprintln!("hardcoded-repo-path-check: rewrote allowlist: {}", path.display());
    Ok(())
}

fn report_stale(declared: &BTreeMap<String, usize>, found: &BTreeMap<String, usize>) -> bool {
    let mut stale = false;
    for (path, want) in declared {
        let got = found.get(path).copied().unwrap_or(0);
        if got == 0 {
            eprintln!("hardcoded-repo-path-check: dead allowlist entry: {path}");
            stale = true;
        } else if got != *want {
            eprintln!("hardcoded-repo-path-check: reference count changed: {path}: declared {want}, found {got}");
            stale = true;
        }
    }
    if stale {
        eprintln!("hardcoded-repo-path-check: refresh listed paths with: north hardcoded-repo-path-check --write-allow");
    }
    stale
}

fn logical_cwd() -> PathBuf {
    let physical = std::env::current_dir().unwrap_or_default();
    std::env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|logical| logical.is_absolute() && logical.canonicalize().ok() == physical.canonicalize().ok())
        .unwrap_or(physical)
}
