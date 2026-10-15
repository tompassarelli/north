use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use regex::Regex;

use crate::error::{NorthError, NorthResult};

pub fn state_dir() -> PathBuf {
    match env::var_os("NORTH_GROUNDING_STATE") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/north/grounding"),
    }
}

const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";

fn prefixed() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?-u:\b)(?:[sS][kK]-[A-Za-z0-9_-]{16,}|[gG][hH][pousrPOUSR]_[A-Za-z0-9]{20,}|[xX][oO][xX][abpABP]-[A-Za-z0-9-]{10,}|[aA][kK][iI][aA][A-Za-z0-9]{16})(?-u:\b)",
        )
        .expect("secret prefixes")
    })
}

fn assignment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?-u:\b)(?:[pP][aA][sS][sS][wW][oO][rR][dD]|[pP][aA][sS][sS][wW][dD]|[tT][oO][kK][eE][nN]|[sS][eE][cC][rR][eE][tT])[{JS_SPACE}]*[:=][{JS_SPACE}]*[^{JS_SPACE}]+"
        ))
        .expect("secret assignments")
    })
}

fn word(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
}

fn token(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-')
}

// JS lookaheads (?![0-9a-f]{40,}\b)(?=…\d)(?=…[A-Za-z]) evaluated by hand: the regex crate has no lookaround.
fn opaque_end(chars: &[char], p: usize) -> Option<usize> {
    let boundary = |i: usize| word(i.checked_sub(1).and_then(|j| chars.get(j))) != word(chars.get(i));
    if !boundary(p) {
        return None;
    }
    let hex = chars[p..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
    if hex >= 40 && !word(chars.get(p + hex)) {
        return None;
    }
    let run = chars[p..].iter().take_while(|c| token(**c)).count();
    let tail = &chars[p..p + run];
    if run < 40 || !tail.iter().any(char::is_ascii_digit) || !tail.iter().any(char::is_ascii_alphabetic) {
        return None;
    }
    (40..=run).rev().map(|k| p + k).find(|&end| boundary(end))
}

pub fn redact(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    offsets.push(text.len());
    let char_at = |byte: usize| offsets.partition_point(|&o| o < byte);
    let mut out = String::with_capacity(text.len());
    let (mut p, mut copied) = (0usize, 0usize);
    let mut next: [Option<Option<(usize, usize)>>; 2] = [None, None];
    while p < chars.len() {
        let at = offsets[p];
        let mut anchored = |slot: usize, re: &Regex| {
            if next[slot].is_some_and(|m| m.is_some_and(|(start, _)| start < at)) || next[slot].is_none() {
                next[slot] = Some(re.find_at(text, at).map(|m| (m.start(), m.end())));
            }
            next[slot].flatten().filter(|(start, _)| *start == at).map(|(_, end)| char_at(end))
        };
        let end = anchored(0, prefixed()).or_else(|| opaque_end(&chars, p)).or_else(|| anchored(1, assignment()));
        match end {
            Some(end) => {
                out.push_str(&text[offsets[copied]..at]);
                out.push_str("[redacted]");
                p = end;
                copied = end;
            }
            None => p += 1,
        }
    }
    out.push_str(&text[offsets[copied]..]);
    out
}

fn sqlite_failure(db: &Path, stderr: &[u8]) -> NorthError {
    NorthError::Configuration(format!("sqlite3 {}: {}", db.display(), String::from_utf8_lossy(stderr).trim()))
}

pub fn sqlite_json(db: &Path, query: &str) -> NorthResult<Vec<serde_json::Value>> {
    let result = Command::new("sqlite3")
        .args(["-readonly", "-json", "-cmd", ".timeout 30000"])
        .arg(db)
        .arg(query)
        .stdin(Stdio::null())
        .output()?;
    if !result.status.success() {
        return Err(sqlite_failure(db, &result.stderr));
    }
    let text = String::from_utf8_lossy(&result.stdout);
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for value in serde_json::Deserializer::from_str(&text).into_iter::<Vec<serde_json::Value>>() {
        rows.extend(value?);
    }
    Ok(rows)
}

pub fn sqlite_exec(db: &Path, script: &str) -> NorthResult<()> {
    let mut child = Command::new("sqlite3")
        .args(["-bail", "-cmd", ".timeout 30000"])
        .arg(db)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().expect("piped stdin").write_all(script.as_bytes())?;
    let result = child.wait_with_output()?;
    if !result.status.success() {
        return Err(sqlite_failure(db, &result.stderr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_secrets_but_keeps_paths_commit_hashes_and_refs() {
        let text = [
            "river gauge password=synthetic-review-only",
            "token: abc123",
            "key sk-abcdefghijklmnopqrstuvwxyz012345",
            "blob Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0LzXcVbNmQwErTy",
            "see harness/skills/orchestrating-codex/references/session-diagnostics.md",
            "landed 4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0 and convo:2216789",
        ]
        .join("\n");
        let out = redact(&text);
        assert!(!out.contains("synthetic-review-only"));
        assert!(!out.contains("abc123"));
        assert!(!out.contains("sk-abcdefghij"));
        assert!(!out.contains("Zx9Qw8Er7Ty6"));
        assert!(out.contains("harness/skills/orchestrating-codex/references/session-diagnostics.md"));
        assert!(out.contains("4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0"));
        assert!(out.contains("convo:2216789"));
    }

    #[test]
    fn sqlite_helpers_write_then_read_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        sqlite_exec(&db, "create table t(a, b); insert into t values (1, 'it''s');").unwrap();
        let rows = sqlite_json(&db, "select a, b from t").unwrap();
        assert_eq!(rows, vec![serde_json::json!({"a": 1, "b": "it's"})]);
        assert!(sqlite_json(&db, "select a from t where a = 2").unwrap().is_empty());
        assert!(sqlite_exec(&db, "insert into missing values (1);").is_err());
    }
}
