use sha2::{Digest, Sha256};

use super::key::NORTH_IGNORES;
use super::path_set::{self, Entry, Kind};
use super::{Store, set_id};

mod artifact;
mod environment;
mod host;
mod tree;

pub use artifact::Artifacts;
pub use environment::{IGNORED, normalized, replace_roots};
pub use tree::Tree;

pub const TARGET: &str = "target";
const LOCKED: [&str; 6] = ["rust-toolchain.toml", "rust-toolchain", "flake.lock", "Cargo.lock", ".cargo/config.toml", ".cargo/config"];
const SELECT_PATHS: &str = "SAFE_PUSH_LAND_SELECT_PATHS";

pub struct Weak<'a> {
    pub unit: &'a str,
    pub line: &'a str,
    pub rustc: &'a str,
    pub environment: &'a [(String, String)],
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn record_name(binary_id: &str, test: &str) -> String {
    hex(format!("{binary_id}\0{test}").as_bytes())
}

pub fn unit_id(binary_id: &str, test: &str) -> String {
    format!("{binary_id} {test}")
}

pub fn without_selection(line: &str) -> String {
    let names: Vec<&str> = NORTH_IGNORES.iter().map(|(name, _)| *name).chain([SELECT_PATHS]).collect();
    let mut kept = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("${") {
        let (before, tail) = rest.split_at(start);
        kept.push_str(before);
        let name_end = tail[2..].find(|character: char| !(character.is_ascii_alphanumeric() || character == '_')).map_or(tail.len(), |end| end + 2);
        let close = tail.find('}').map_or(tail.len(), |close| close + 1);
        if names.contains(&&tail[2..name_end]) {
            rest = &tail[close..];
        } else {
            kept.push_str(&tail[..close]);
            rest = &tail[close..];
        }
    }
    kept.push_str(rest);
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn weak(input: &Weak, tree: &Tree) -> String {
    let mut text = format!("unit\0{}\nline\0{}\nrustc\0{}\n", input.unit, input.line, input.rustc);
    for path in LOCKED {
        text.push_str(&format!("file\0{path}\0{}\n", tree.content(path)));
    }
    for (name, value) in input.environment {
        text.push_str(&format!("env\0{name}\0{value}\n"));
    }
    hex(text.as_bytes())
}

pub fn strong(weak: &str, entries: &[Entry], tree: &Tree, artifacts: &mut Artifacts) -> Option<String> {
    let mut text = format!("weak\0{weak}\n");
    for entry in entries {
        let answer = if entry.path.starts_with('/') {
            host::answer(entry.kind, &entry.path)?
        } else {
            match entry.path.strip_prefix(TARGET) {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => artifacts.answer(tree, rest.trim_start_matches('/'))?,
                _ => tree.answer(entry.kind, &entry.path)?,
            }
        };
        text.push_str(&format!("{:?}\0{}\0{answer}\n", entry.kind, entry.path));
    }
    Some(hex(text.as_bytes()))
}

pub fn first_difference(left: &[Entry], right: &[Entry]) -> Option<String> {
    let (only_left, only_right) = (left.iter().find(|entry| !right.contains(entry)), right.iter().find(|entry| !left.contains(entry)));
    only_left.or(only_right).map(|entry| format!("{} {}", entry.kind_word(), entry.path))
}

pub fn reusable(entries: &[Entry]) -> bool {
    !entries.iter().any(|entry| entry.kind == Kind::Unreusable)
}

pub fn reusable_set(store: &Store, weak: &str, tree: &Tree, artifacts: &mut Artifacts) -> Option<(String, Vec<Entry>)> {
    for bytes in store.sets(weak).ok()? {
        let Ok(entries) = path_set::parse(&String::from_utf8_lossy(&bytes)) else { continue };
        let Some(strong) = strong(weak, &entries, tree, artifacts) else { continue };
        if store.pass(weak, &strong).ok().flatten().is_some() {
            return Some((set_id(&bytes), entries));
        }
    }
    None
}
