//! The tracked files a worker's brief names, read from its lane's base commit (origin/main, else main)
//! where the session runs, here or over ssh on its build host, and appended to the brief, so its first
//! model request edits instead of reading them.
use std::path::Path;
use std::process::Command;

use north_base::platform::process::try_run_command_within;
use north_fleet_store::ssh::{SSH_WAIT, quote};

use super::model::Machine;

pub const HEADING: &str = "Files the brief names";
const FILE_CAP: usize = 8 * 1024;
const TOTAL_CAP: usize = 32 * 1024;
const BASE: &str = "base=$(git rev-parse -q --verify 'refs/remotes/origin/main^{commit}' || git rev-parse -q --verify 'refs/heads/main^{commit}') && echo \"$base\"";

/// `brief` with the content and blob id of every tracked file it names in `cwd`'s repository, at most
/// 8 KB a file and 32 KB in all, and a line for each named file left out; `brief` itself when it names
/// none or `cwd` holds no repository with a base to read.
pub fn shown(brief: &str, cwd: &Path, machine: Option<&Machine>) -> String {
    let named = named(brief, cwd);
    if named.is_empty() {
        return brief.to_owned();
    }
    let paths = named.iter().map(|path| quote(path)).collect::<Vec<_>>().join(" ");
    let script = format!("{BASE} && git ls-tree -z -l \"$base\" -- {paths} && printf '\\0' && git ls-tree -l \"$base\" -- {paths} | awk '$2 == \"blob\" && $4 <= {FILE_CAP} {{ print $3 }}' | git cat-file --batch");
    let Some(listed) = run(machine, cwd, &script) else { return brief.to_owned() };
    let Some((base, entries, batch)) = parted(&listed) else { return brief.to_owned() };
    let tracked: Vec<Entry> = entries.split('\0').filter_map(Entry::parse).collect();
    let (taken, mut left) = chosen(&named, &tracked, base);
    let contents = blobs(batch);
    let mut section = format!("{HEADING}, read from the lane's base commit {base}; they need no reading:");
    for entry in &taken {
        match contents.iter().find(|(blob, _)| *blob == entry.blob).and_then(|(_, bytes)| std::str::from_utf8(bytes).ok()) {
            Some(text) => section.push_str(&format!("\n=== {} (blob {})\n{}", entry.path, entry.blob, text.trim_end_matches('\n'))),
            None => left.push(format!("{} is not text", entry.path)),
        }
    }
    for line in &left {
        section.push_str(&format!("\nLeft out: {line}"));
    }
    format!("{brief}\n\n{section}")
}

fn chosen<'a>(named: &[String], tracked: &'a [Entry], base: &str) -> (Vec<&'a Entry>, Vec<String>) {
    let (mut taken, mut left, mut total) = (Vec::new(), Vec::new(), 0);
    for path in named {
        let Some(entry) = tracked.iter().find(|entry| entry.path == *path) else {
            if path.contains('/') {
                left.push(format!("{path} is not tracked at {base}"));
            }
            continue;
        };
        if entry.size > FILE_CAP {
            left.push(format!("{path} is over 8 KB ({} bytes)", entry.size));
        } else if total + entry.size > TOTAL_CAP {
            left.push(format!("{path} would take the files shown past 32 KB"));
        } else {
            total += entry.size;
            taken.push(entry);
        }
    }
    (taken, left)
}

struct Entry {
    blob: String,
    size: usize,
    path: String,
}

impl Entry {
    fn parse(line: &str) -> Option<Self> {
        let (head, path) = line.split_once('\t')?;
        let [mode, kind, blob, size]: [&str; 4] = head.split_whitespace().collect::<Vec<_>>().try_into().ok()?;
        if kind != "blob" || mode == "120000" {
            return None;
        }
        Some(Self { blob: blob.to_owned(), size: size.parse().ok()?, path: path.to_owned() })
    }
}

fn named(brief: &str, cwd: &Path) -> Vec<String> {
    let root = format!("{}/", cwd.display());
    let mut paths: Vec<String> = Vec::new();
    for word in brief.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '`' | '(' | ')' | '"' | '\'' | '<' | '>' | '[' | ']')) {
        let word = word.trim_end_matches(['.', ':']);
        let word = word.strip_prefix(root.as_str()).unwrap_or(word);
        if relative_file(word) && !paths.iter().any(|known| known == word) {
            paths.push(word.to_owned());
        }
    }
    paths
}

fn relative_file(word: &str) -> bool {
    let Some((_, extension)) = word.rsplit_once('.') else { return false };
    let plain = |part: &str| !part.is_empty() && part != "." && part != ".." && part.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    (1..=5).contains(&extension.len()) && extension.starts_with(|c: char| c.is_ascii_alphabetic()) && word.split('/').all(plain)
}

fn parted(listed: &[u8]) -> Option<(&str, &str, &[u8])> {
    let newline = listed.iter().position(|byte| *byte == b'\n')?;
    let (base, rest) = (std::str::from_utf8(&listed[..newline]).ok()?, &listed[newline + 1..]);
    let end = rest.windows(2).position(|pair| pair == b"\0\0").map_or(rest.len(), |at| at + 1);
    Some((base, std::str::from_utf8(&rest[..end]).ok()?, rest.get(end + 1..).unwrap_or_default()))
}

fn blobs(batch: &[u8]) -> Vec<(String, &[u8])> {
    let mut found = Vec::new();
    let mut rest = batch;
    while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
        let header = String::from_utf8_lossy(&rest[..end]).into_owned();
        let mut fields = header.split_whitespace();
        let (Some(blob), Some(size)) = (fields.next(), fields.nth(1).and_then(|size| size.parse::<usize>().ok())) else { break };
        let Some(content) = rest.get(end + 1..end + 1 + size) else { break };
        found.push((blob.to_owned(), content));
        rest = rest.get(end + 2 + size..).unwrap_or_default();
    }
    found
}

fn run(machine: Option<&Machine>, cwd: &Path, script: &str) -> Option<Vec<u8>> {
    let script = format!("cd {} && {script}", quote(&cwd.to_string_lossy()));
    let command = match machine {
        None => {
            let mut sh = Command::new("sh");
            sh.args(["-c", &script]);
            sh
        }
        Some(machine) => {
            let mut ssh = Command::new(super::runtime::binary("ssh"));
            ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &machine.ssh, "--", &script]);
            ssh
        }
    };
    let output = try_run_command_within(command, None, SSH_WAIT).ok().flatten()?;
    output.status.success().then_some(output.stdout)
}

#[cfg(test)]
#[path = "brief_files_tests.rs"]
mod tests;
