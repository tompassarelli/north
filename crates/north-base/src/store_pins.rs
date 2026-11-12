//! The store entries a commit pins: a `STORE@PINS` host input in its committed `.safe-push` names a store and a pins directory of the commit, one file per input family or one directory of shard files per sharded family, each file holding the 64-hex name of the store entry that family's build reads.
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ENTRY_LEN: usize = 64;
const STORE_ENTRY: char = '@';
const SEPARATOR: &str = " :: ";

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new("git").arg("-C").arg(repo).args(arguments).output() else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

pub fn store_input(input: &str) -> Option<(&str, &str)> {
    let (store, pins) = input.split_once(STORE_ENTRY)?;
    (!store.is_empty() && !pins.is_empty() && !pins.starts_with('/')).then_some((store, pins))
}

/// Each `FAMILY/ENTRY` the pins directory names at `head`, where an entry is the first 64-hex name in a family's file or in each shard file of a sharded family; a pin file that names none is left out so the probe reports the store entry missing.
pub fn pinned_entries(repo: &Path, head: &str, pins: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for pin in git_lines(repo, &["ls-tree", "-r", "--name-only", &format!("{head}:{pins}")]) {
        let family = pin.split('/').next().unwrap_or_default();
        if family.is_empty() || !family.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)) {
            continue;
        }
        let text = git_lines(repo, &["show", &format!("{head}:{pins}/{pin}")]).join("\n");
        let Some(entry) = text.split(|character: char| !character.is_ascii_alphanumeric()).find(|word| word.len() == ENTRY_LEN && word.bytes().all(|byte| byte.is_ascii_hexdigit())) else { continue };
        let named = format!("{family}/{entry}");
        if !entries.contains(&named) {
            entries.push(named);
        }
    }
    entries
}

/// Every store entry directory the `STORE@PINS` host inputs of `head`'s committed `.safe-push` pin, a `~/` store read against `home`.
pub fn pinned_store_entries(repo: &Path, head: &str, home: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for line in git_lines(repo, &["config", "--blob", &format!("{head}:.safe-push"), "--get-all", "land.host-input"]) {
        let Some((inputs, _)) = line.split_once(SEPARATOR) else { continue };
        for (store, pins) in inputs.split_whitespace().filter_map(store_input) {
            let root = store.strip_prefix("~/").map_or_else(|| PathBuf::from(store), |below| home.join(below));
            for entry in pinned_entries(repo, head, pins).into_iter().map(|entry| root.join(entry)) {
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}
