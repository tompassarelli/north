use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use toml_edit::DocumentMut;

pub const TREE: &str = "tree";
pub const DEFAULTS: &str = "harness/defaults.toml";
pub const MODEL_CATALOG: &str = "harness/model-catalog.toml";

static TABLES: LazyLock<Mutex<BTreeMap<&'static str, &'static str>>> = LazyLock::new(Mutex::default);

pub fn table(relative: &'static str) -> &'static str {
    let mut tables = TABLES.lock().unwrap_or_else(PoisonError::into_inner);
    tables.entry(relative).or_insert_with(|| {
        let root = harness_root().unwrap_or_else(|skew| panic!("{skew}"));
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("north: cannot read {}: {error}", path.display()));
        Box::leak(text.into_boxed_str())
    })
}

static DOCUMENTS: LazyLock<Mutex<BTreeMap<&'static str, &'static DocumentMut>>> = LazyLock::new(Mutex::default);

/// The harness table at `relative` parsed as TOML, once per process however many crates read it:
/// `harness/defaults.toml` alone takes over a millisecond to parse.
pub fn document(relative: &'static str) -> &'static DocumentMut {
    let mut documents = DOCUMENTS.lock().unwrap_or_else(PoisonError::into_inner);
    documents.entry(relative).or_insert_with(|| Box::leak(Box::new(table(relative).parse().unwrap_or_else(|error| panic!("north: {relative} does not parse: {error}")))))
}

pub fn harness_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().unwrap_or_default();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    resolve(std::env::var_os("NORTH_HOME").map(PathBuf::from), &executable, &home)
}

pub fn is_sha(name: &str) -> bool {
    name.len() == 40 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn resolve(north_home: Option<PathBuf>, executable: &Path, home: &Path) -> Result<PathBuf, String> {
    if let Some(root) = north_home {
        return Ok(root);
    }
    if let Some(root) = own_tree(executable).or_else(|| promoted_sha(executable).is_none().then(|| source_tree(Path::new(env!("CARGO_MANIFEST_DIR")))).flatten()) {
        return Ok(root);
    }
    checkout_for(executable, home.join("code/north/main"))
}

fn own_tree(executable: &Path) -> Option<PathBuf> {
    let promoted = executable.parent().map(|runtime| runtime.join(TREE));
    promoted.into_iter().chain(executable.ancestors().map(Path::to_path_buf)).find(|root| root.join(DEFAULTS).is_file())
}

fn source_tree(manifest: &Path) -> Option<PathBuf> {
    manifest.ancestors().nth(2).map(Path::to_path_buf).filter(|root| root.join(DEFAULTS).is_file())
}

fn checkout_for(executable: &Path, checkout: PathBuf) -> Result<PathBuf, String> {
    let Some(runtime) = promoted_sha(executable) else {
        return Ok(checkout);
    };
    let head = head(&checkout).unwrap_or_default();
    match head == runtime {
        true => Ok(checkout),
        false => Err(format!(
            "north: runtime {runtime} has no tree and the checkout {} is at {}; run north runtime update",
            checkout.display(),
            if head.is_empty() { "no commit" } else { &head }
        )),
    }
}

pub fn revision(root: &Path) -> Option<String> {
    tree_sha(root).map(str::to_owned).or_else(|| head(root))
}

fn head(checkout: &Path) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(checkout).args(["rev-parse", "HEAD"]).output().ok()?;
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !head.is_empty()).then_some(head)
}

fn tree_sha(root: &Path) -> Option<&str> {
    (root.file_name()? == TREE).then_some(())?;
    runtime_sha(root.parent()?)
}

/// The sha of the promoted runtime `executable` runs from, None outside `runtimes/SHA/`.
pub fn promoted_runtime(executable: &Path) -> Option<&str> {
    promoted_sha(executable)
}

fn promoted_sha(executable: &Path) -> Option<&str> {
    runtime_sha(executable.parent()?)
}

fn runtime_sha(runtime: &Path) -> Option<&str> {
    (runtime.parent()?.file_name()? == "runtimes").then_some(())?;
    runtime.file_name()?.to_str().filter(|name| is_sha(name))
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
