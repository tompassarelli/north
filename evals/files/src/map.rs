use std::collections::BTreeMap;
use std::path::Path;

use crate::proc::git;
use crate::task::Tree;

const SOURCE: &[&str] = &["ts", "tsx", "mts", "mjs", "js", "cjs", "rs", "py", "sh", "lua", "toml", "md", "json", "yaml", "yml", "tsv", "txt"];
const VENDORED: &[&str] = &["repos/", "node_modules/", "vendor/", "third_party/", "third-party/"];
const SYMBOL_LIMIT: usize = 6;
const READ_LIMIT: u64 = 64 * 1024;
pub const MAP_BYTES: usize = 240 * 1024;

pub fn mapped(path: &str) -> bool {
    let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    SOURCE.contains(&extension) && !VENDORED.iter().any(|dir| path.starts_with(dir) || path.contains(&format!("/{dir}")))
}

fn declared(line: &str) -> Option<String> {
    let mut rest = line.trim_start();
    if let Some(title) = rest.strip_prefix("# ") {
        return Some(title.trim().chars().take(60).collect());
    }
    for keyword in ["export ", "pub(crate) ", "pub ", "default ", "async ", "declare "] {
        rest = rest.strip_prefix(keyword).unwrap_or(rest);
    }
    for keyword in ["function*", "function", "class", "const", "interface", "type", "enum", "fn", "struct", "trait", "impl", "def"] {
        if let Some(after) = rest.strip_prefix(keyword).and_then(|tail| tail.strip_prefix(' ')) {
            let name: String = after.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
            return (name.len() > 1).then_some(name);
        }
    }
    None
}

pub fn symbols(content: &str, path: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in content.lines().filter(|line| path.ends_with(".md") == line.starts_with('#')).filter_map(declared) {
        if !names.contains(&name) {
            names.push(name);
        }
        if names.len() == SYMBOL_LIMIT {
            break;
        }
    }
    names
}

pub fn symbols_by_path(repo: &Path, base: &str, tree: &Tree) -> Result<BTreeMap<String, Vec<String>>, String> {
    let wanted: Vec<&String> = tree.sizes.iter().filter(|(path, size)| mapped(path) && **size <= READ_LIMIT).map(|(path, _)| path).collect();
    let request: String = wanted.iter().map(|path| format!("{base}:{path}\n")).collect();
    let out = git(repo, &["cat-file", "--batch"], Some(request.into_bytes()))?.out;
    let (mut at, mut found) = (0, BTreeMap::new());
    for path in wanted {
        let Some(newline) = out[at..].iter().position(|&b| b == b'\n') else { break };
        let header = String::from_utf8_lossy(&out[at..at + newline]).into_owned();
        at += newline + 1;
        let size: usize = header.split_whitespace().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if header.ends_with("missing") {
            continue;
        }
        found.insert(path.clone(), symbols(&String::from_utf8_lossy(&out[at..at + size]), path));
        at += size + 1;
    }
    Ok(found)
}

pub fn entry(path: &str, symbols: &[String], keep: usize) -> String {
    if symbols.is_empty() || keep == 0 { path.to_owned() } else { format!("{path}: {}", symbols.iter().take(keep).cloned().collect::<Vec<_>>().join(", ")) }
}

fn is_test(path: &str) -> bool {
    path.contains("test") || path.contains("spec")
}

pub fn repository_map(tree: &Tree, symbols: &BTreeMap<String, Vec<String>>) -> String {
    let paths: Vec<&String> = tree.sizes.keys().filter(|path| mapped(path)).collect();
    let none = Vec::new();
    let render = |keep_test: usize, keep: usize| -> String {
        paths.iter().map(|path| entry(path, symbols.get(*path).unwrap_or(&none), if is_test(path) { keep_test } else { keep })).collect::<Vec<_>>().join("\n")
    };
    [(SYMBOL_LIMIT, SYMBOL_LIMIT), (0, SYMBOL_LIMIT), (0, 3), (0, 0)].into_iter().map(|(t, k)| render(t, k)).find(|text| text.len() <= MAP_BYTES).unwrap_or_else(|| render(0, 0))
}
