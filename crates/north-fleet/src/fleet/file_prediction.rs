//! The files a worker's brief does not name, predicted from the brief and the repository's map at the lane's base commit
//! (north#1234; experiment G1 of north#1147 scored this classifier naming a changed file in its top 5 on 81% of 120 closed
//! boxes against text search's 22%). The classifier is `north check oneshot` on a small model with no tools; it sees the
//! brief and the tracked files' paths with the names each declares. The files it names go in the first prompt under the
//! caps `brief_files` applies, and the launch records them.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::process::try_run_command_within;

use super::brief_files;
use super::model::{Machine, Predicted};

pub const HEADING: &str = "Files the launch predicts the task changes, from the brief and the repository's map; the brief names none, so treat these as a guess to confirm, not a list to stay inside. Read from the lane's base commit";
const MODEL: &str = "claude-haiku-5-5";
const SYSTEM: &str = "You predict which files a coding task will change in a repository. You see only the task text and a list of the repository's files. Answer with at most 10 repository paths, one per line, the most likely first, copied exactly from the list. Write nothing else.";
const MOST_FILES: usize = 10;
const SOURCE: &[&str] = &["ts", "tsx", "mts", "mjs", "js", "cjs", "rs", "py", "sh", "lua", "toml", "md", "json", "yaml", "yml", "tsv", "txt"];
const VENDORED: &[&str] = &["repos/", "node_modules/", "vendor/", "third_party/", "third-party/"];
const SYMBOL_LIMIT: usize = 6;
const MAP_BYTES: usize = 240 * 1024;
const ASK_WAIT: Duration = Duration::from_secs(120);
const BASE: &str = "base=$(git rev-parse -q --verify 'refs/remotes/origin/main^{commit}' || git rev-parse -q --verify 'refs/heads/main^{commit}')";
const DECLARATION: &str = r"^[[:space:]]*((pub(\([a-z]+\))?|export|async|default|declare)[[:space:]]+)*(function\*?|class|const|interface|type|enum|fn|struct|trait|impl|def)[[:space:]]+[A-Za-z0-9_$]+|^# ";

/// The first-prompt section for the files `brief` does not name, with what was predicted; `None` when the brief names files, or
/// `cwd` holds no repository with a base to map.
pub fn section(brief: &str, cwd: &Path, machine: Option<&Machine>, north: &Path) -> Option<(Option<String>, Predicted)> {
    if !brief_files::named(brief, cwd).is_empty() {
        return None;
    }
    let (tree, symbols) = (tree(cwd, machine)?, symbols(cwd, machine)?);
    let map = repository_map(&tree, &symbols);
    let writes = parse(&ask(north, &format!("Task:\n{brief}\n\nThe repository's files at the task's base commit, each with the names it declares:\n{map}\n"))?, &tree);
    let read = brief_files::read(&writes, cwd, machine, HEADING);
    let reads = read.as_ref().map(|(_, carried)| carried.clone()).unwrap_or_default();
    Some((read.map(|(section, _)| section), Predicted { writes, reads }))
}

fn tree(cwd: &Path, machine: Option<&Machine>) -> Option<BTreeMap<String, u64>> {
    let listed = brief_files::run(machine, cwd, &format!("{BASE} && git ls-tree -r -l -z \"$base\""))?;
    Some(
        String::from_utf8_lossy(&listed)
            .split('\0')
            .filter_map(|entry| {
                let (head, path) = entry.split_once('\t')?;
                let size = head.split_whitespace().nth(3)?.parse().ok()?;
                Some((path.to_owned(), size))
            })
            .collect(),
    )
}

fn symbols(cwd: &Path, machine: Option<&Machine>) -> Option<BTreeMap<String, Vec<String>>> {
    let script = format!("{BASE} && git grep -I -z -E '{DECLARATION}' \"$base\" | head -c 16000000");
    let found = brief_files::run(machine, cwd, &script)?;
    let mut by_path: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let text = String::from_utf8_lossy(&found).into_owned();
    let mut parts = text.split('\0');
    let mut head = parts.next();
    while let Some(labelled) = head {
        let Some(rest) = parts.next() else { break };
        let (line, next) = rest.split_once('\n').map_or((rest, None), |(line, next)| (line, Some(next)));
        let path = labelled.split_once(':').map_or(labelled, |(_, path)| path);
        if let Some(name) = declared(line, path.ends_with(".md")) {
            let names = by_path.entry(path.to_owned()).or_default();
            if names.len() < SYMBOL_LIMIT && !names.contains(&name) {
                names.push(name);
            }
        }
        head = next;
    }
    Some(by_path)
}

fn mapped(path: &str) -> bool {
    let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
    SOURCE.contains(&extension) && !VENDORED.iter().any(|dir| path.starts_with(dir) || path.contains(&format!("/{dir}")))
}

fn declared(line: &str, markdown: bool) -> Option<String> {
    let mut rest = line.trim_start();
    if let Some(title) = rest.strip_prefix("# ") {
        return markdown.then(|| title.trim().chars().take(60).collect());
    }
    if markdown {
        return None;
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

fn repository_map(tree: &BTreeMap<String, u64>, symbols: &BTreeMap<String, Vec<String>>) -> String {
    let paths: Vec<&String> = tree.keys().filter(|path| mapped(path)).collect();
    let render = |keep: usize| -> String {
        paths
            .iter()
            .map(|path| match symbols.get(*path).filter(|names| keep > 0 && !names.is_empty()) {
                Some(names) => format!("{path}: {}", names.iter().take(keep).cloned().collect::<Vec<_>>().join(", ")),
                None => (*path).clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    [SYMBOL_LIMIT, 3, 0].into_iter().map(render).find(|text| text.len() <= MAP_BYTES).unwrap_or_else(|| render(0))
}

fn ask(north: &Path, input: &str) -> Option<String> {
    let mut command = Command::new(north);
    command.args(["check", "oneshot", "--model", MODEL, "--effort", "low", "--system", SYSTEM, "--json"]).stderr(Stdio::null());
    let output = try_run_command_within(command, Some(input.as_bytes()), ASK_WAIT).ok().flatten()?;
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    reply["text"].as_str().map(str::to_owned)
}

fn parse(answer: &str, tree: &BTreeMap<String, u64>) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for line in answer.lines() {
        let path = line.trim().trim_start_matches(|c: char| c.is_ascii_digit() || ".-*) ".contains(c)).trim_matches('`').split(':').next().unwrap_or("").trim();
        if tree.contains_key(path) && !files.iter().any(|seen| seen == path) && files.len() < MOST_FILES {
            files.push(path.to_owned());
        }
    }
    files
}
