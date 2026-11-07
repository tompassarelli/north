use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

const SOURCES: [&str; 2] = ["src", "crates"];
const TEST_SOURCES: [&str; 3] = ["_tests.rs", "/tests.rs", "/tests/"];
const GLUE: &str = r"(?:[\s,()\[\]&?.]|to_owned\(\)|into\(\)|to_string\(\)|display\(\)|to_string_lossy\(\)|into_owned\(\)|unwrap\(\)|unwrap_or_default\(\)|clone\(\)|arg\(|args\(|extend\(|push\(|vec!|String::from|OsString::from)*";

static LAUNCHER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""north"|north_binary\(\)|current_exe\(\)|\bnorth\b"#).unwrap_or_else(|error| unreachable!("{error}")));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r#"\A{GLUE}"([a-z][a-z0-9-]*)""#)).unwrap_or_else(|error| unreachable!("{error}")));
static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:"|[\s;&|(`'])(?:north|\{[a-z_]*north[a-z_]*\}) ([a-z][a-z0-9-]*(?: [a-z][a-z0-9-]*)?)"#).unwrap_or_else(|error| unreachable!("{error}")));

#[derive(Debug, Eq, PartialEq)]
pub struct Call {
    pub line: usize,
    pub words: Vec<String>,
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].bytes().filter(|byte| *byte == b'\n').count() + 1
}

pub fn calls(text: &str) -> Vec<Call> {
    let mut found = Vec::new();
    for launcher in LAUNCHER.find_iter(text) {
        let mut words = Vec::new();
        let mut at = launcher.end();
        while let Some(word) = WORD.captures(&text[at..]) {
            let (whole, name) = (word.get(0), word.get(1));
            let (Some(whole), Some(name)) = (whole, name) else { break };
            words.push(name.as_str().to_owned());
            at += whole.end();
        }
        if !words.is_empty() {
            found.push(Call { line: line_of(text, launcher.start()), words });
        }
    }
    for line in LINE.captures_iter(text) {
        if let (Some(whole), Some(words)) = (line.get(0), line.get(1)) {
            found.push(Call { line: line_of(text, whole.start()), words: words.as_str().split(' ').map(str::to_owned).collect() });
        }
    }
    found
}

fn is_north_code(path: &str) -> bool {
    path.ends_with(".rs") && SOURCES.iter().any(|root| path.starts_with(&format!("{root}/"))) && !TEST_SOURCES.iter().any(|marker| path.contains(marker))
}

pub fn files(repo: &Path, rev: &str) -> Result<Vec<(String, String)>, String> {
    let listing = Command::new("git").arg("-C").arg(repo).args(["ls-tree", "-r", "--name-only", rev]).output().map_err(|error| format!("git ls-tree: {error}"))?;
    if !listing.status.success() {
        return Err(format!("git ls-tree {rev} failed: {}", String::from_utf8_lossy(&listing.stderr).trim()));
    }
    let paths: Vec<String> = String::from_utf8_lossy(&listing.stdout).lines().filter(|path| is_north_code(path)).map(str::to_owned).collect();
    let mut batch = Command::new("git").arg("-C").arg(repo).args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|error| format!("git cat-file: {error}"))?;
    let requests: String = paths.iter().map(|path| format!("{rev}:{path}\n")).collect();
    let mut stdin = batch.stdin.take().ok_or("git cat-file has no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(requests.as_bytes()));
    let output = batch.wait_with_output().map_err(|error| format!("git cat-file: {error}"))?;
    writer.join().map_err(|_| "git cat-file writer panicked")?.map_err(|error| format!("git cat-file: {error}"))?;
    let mut rest = output.stdout.as_slice();
    let mut contents = Vec::new();
    for path in paths {
        let Some(newline) = rest.iter().position(|byte| *byte == b'\n') else { break };
        let size = String::from_utf8_lossy(&rest[..newline]).split(' ').nth(2).and_then(|size| size.parse::<usize>().ok()).ok_or_else(|| format!("{rev}:{path} is not a file at {rev}"))?;
        let body = rest.get(newline + 1..newline + 1 + size).ok_or("git cat-file output ended early")?;
        contents.push((path, String::from_utf8_lossy(body).into_owned()));
        rest = rest.get(newline + 2 + size..).unwrap_or_default();
    }
    Ok(contents)
}

#[cfg(test)]
#[path = "rust_calls_tests.rs"]
mod tests;
