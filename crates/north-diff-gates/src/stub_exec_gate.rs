use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::git as platform_git;

type Result<T> = std::result::Result<T, String>;

pub type Allowlist = BTreeMap<String, String>;

pub const ALLOWLIST: &str = ".stub-exec-allowlist.json";
pub const ROOTS: [&str; 3] = ["crates", "src", "tests"];
const REMEDY: &str = "write executable test stubs with north_core::foundation::shim::write_script or write_executable and copy binaries with copy_executable";

static SITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"from_mode\(0o7[0-7]{2}\)|fn executable\(path: &Path, |fs::copy\(\s*(?:concat!\(env!\("CARGO_MANIFEST_DIR"\), "/bin|Path::new\(env!\("CARGO_MANIFEST_DIR"\)\)\.join\("bin|env!\("CARGO_BIN_EXE)"#).expect("site pattern")
});

pub fn sites(text: &str) -> usize {
    SITE.find_iter(text).count()
}

pub fn allowlist(bytes: &[u8]) -> Result<Allowlist> {
    serde_json::from_slice(bytes).map_err(|error| format!("{ALLOWLIST}: {error}"))
}

pub fn problems(files: &BTreeMap<String, String>, allowed: &Allowlist) -> Vec<String> {
    let unallowed = files
        .iter()
        .filter(|(path, text)| sites(text) > 0 && !allowed.contains_key(*path))
        .map(|(path, _)| format!("{path}: makes a file executable outside the shim helpers; {REMEDY}"));
    let unneeded = allowed
        .iter()
        .filter(|(path, _)| files.get(*path).is_none_or(|text| sites(text) == 0))
        .map(|(path, reason)| format!("{ALLOWLIST}: {path} ({reason}) no longer makes a file executable; remove its entry"));
    unallowed.chain(unneeded).collect()
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    platform_git::stdout(repo, args, |output| format!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim()))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn blob(repo: &Path, head: &str, path: &str) -> Result<Option<String>> {
    if git(repo, &["ls-tree", "--name-only", head, "--", path])?.is_empty() {
        return Ok(None);
    }
    git(repo, &["show", &format!("{head}:{path}")]).map(Some)
}

pub fn violations(repo: &Path, destination: &str, base: &str, head: &str) -> Result<Vec<String>> {
    let carried = blob(repo, head, ALLOWLIST)?;
    let lane_predates_policy = carried.is_none();
    let Some(listing) = carried.or(blob(repo, destination, ALLOWLIST)?) else { return Ok(Vec::new()) };
    let allowed = allowlist(listing.as_bytes())?;
    let mut arguments = vec!["diff", "--name-only", "--no-renames", base, head, "--", ALLOWLIST];
    arguments.extend(ROOTS);
    let changed: BTreeSet<String> = git(repo, &arguments)?.lines().map(str::to_owned).collect();
    let relisted = !lane_predates_policy && changed.contains(ALLOWLIST);
    let checked: Allowlist = allowed.into_iter().filter(|(path, _)| relisted || changed.contains(path)).collect();
    let mut files = BTreeMap::new();
    for path in changed.iter().filter(|path| path.ends_with(".rs")).chain(checked.keys()) {
        if let Some(text) = blob(repo, head, path)? {
            files.insert(path.clone(), text);
        }
    }
    Ok(problems(&files, &checked))
}

pub fn check(repo: &Path, destination: &str, base: &str, head: &str) -> bool {
    match violations(repo, destination, base, head) {
        Ok(found) if found.is_empty() => true,
        Ok(found) => {
            for line in &found {
                println!("stub-exec-gate: {line}");
            }
            false
        }
        Err(error) => {
            eprintln!("stub-exec-gate: {error}");
            false
        }
    }
}
