use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

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

const FILE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "json", "toml", "yaml", "yml", "rs", "ts", "tsx", "js", "jsx", "py", "sh", "sql", "lock", "log", "html", "css", "csv", "png", "jpg", "jpeg", "gif", "svg", "pdf", "patch", "diff",
];

fn file_extension_follows(chars: &[char], i: usize) -> bool {
    if chars.get(i) != Some(&'.') {
        return false;
    }
    let ext: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_alphanumeric()).collect();
    !word(chars.get(i + 1 + ext.len())) && FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
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
    if file_extension_follows(chars, p + run) {
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

pub const SQLITE_BUSY: Duration = Duration::from_secs(30);

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
            "see harness/skills/workers/references/codex-sessions.md",
            "landed 4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0 and convo:2216789",
        ]
        .join("\n");
        let out = redact(&text);
        assert!(!out.contains("synthetic-review-only"));
        assert!(!out.contains("abc123"));
        assert!(!out.contains("sk-abcdefghij"));
        assert!(!out.contains("Zx9Qw8Er7Ty6"));
        assert!(out.contains("harness/skills/workers/references/codex-sessions.md"));
        assert!(out.contains("4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0"));
        assert!(out.contains("convo:2216789"));
    }

    #[test]
    fn redact_keeps_long_digit_bearing_filenames_but_masks_opaque_tokens() {
        let text = [
            "see docs/reports/gjoa-2026-10-27-ci-run-38240508386-diagnosis-summary.md",
            "notes 2026-10-27-long-generated-report-name-r7q9w2e8t5y1u4i0o3p6.txt",
            "blob Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0LzXcVbNmQwErTy.then",
            "jwt eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9Q8w2E.Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0LzXcVbNmQw.dozjgNryP4J3jVmNHl0w5NXgLa7Kq2Mz9Tb1Vc8Xd6Rf",
        ]
        .join("\n");
        let out = redact(&text);
        assert!(out.contains("docs/reports/gjoa-2026-10-27-ci-run-38240508386-diagnosis-summary.md"));
        assert!(out.contains("2026-10-27-long-generated-report-name-r7q9w2e8t5y1u4i0o3p6.txt"));
        assert!(!out.contains("Zx9Qw8Er7Ty6"));
        assert!(!out.contains("Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0LzXcVbNmQw."));
        assert!(!out.contains("dozjgNryP4J3"));
    }
}
