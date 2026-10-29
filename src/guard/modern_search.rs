mod find;
mod grep;
mod rewrite;

use super::search_words::{self, Located, MAX_NESTING};
use crate::hook::{HookInput, Verdict};
use crate::shell;

const GREP_LIKE: &[&str] = &["grep", "egrep", "fgrep", "rgrep"];
const UNIT: &str = "modern-search-guard";
const REWRITE_NOTE: &str = "modern-search-guard: rewrote a recursive grep or find tree search to rg or fd before it ran, keeping every shell argument and the rest of the command. rg and fd skip hidden and git-ignored files; add --hidden or -uu (rg), or -H and -I (fd), when you need them. Write rg and fd directly next time.";

struct Hit {
    tool: String,
    original: String,
    replacement: String,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some((command, cwd)) = search_words::bash_call(input) else {
        return Verdict::Allow;
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let Some(decision) = decide(command, &cwd, &home) else {
        return Verdict::Allow;
    };
    if crate::hook::authoring_guards_off(UNIT) {
        return Verdict::Allow;
    }
    match decision {
        Decision::Deny(reason) => Verdict::Deny(reason),
        Decision::Rewrite(candidate) => {
            let mut updated = input.tool_input.clone();
            updated["command"] = candidate.as_str().into();
            Verdict::Rewrite(updated, REWRITE_NOTE.into())
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Decision {
    Deny(String),
    Rewrite(String),
}

pub(super) fn decide(command: &str, cwd: &str, home: &str) -> Option<Decision> {
    let hit = inspect_command(command, cwd, home, 0)?;
    let candidates = [
        exact_pattern_file_rewrite(command, cwd, home),
        rewrite::rewrite(command, cwd, home),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate != command && inspect_command(&candidate, cwd, home, 0).is_none() {
            return Some(Decision::Rewrite(candidate));
        }
    }
    Some(Decision::Deny(reason(&hit)))
}

fn reason(hit: &Hit) -> String {
    let (slow, hidden) = if hit.tool.ends_with("find") {
        (
            "a `find` tree search",
            "fd skips hidden and git-ignored files; add -H and/or -I when you need them.",
        )
    } else {
        (
            "a recursive grep",
            "rg skips hidden and git-ignored files; add --hidden or -uu when you need them.",
        )
    };
    format!(
        "BLOCKED: `{}` is {slow}; use the modern tool instead: `{}`. {hidden} Defaults: rg for content search, fd for finding files, ast-grep for structural code search, sd for simple replacements, jq/yq for JSON/YAML.",
        hit.original, hit.replacement
    )
}

fn render(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            let safe = !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./~${}-".contains(&b));
            if safe {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn inspect_command(text: &str, base: &str, home: &str, depth: usize) -> Option<Hit> {
    let tokens = shell::tokenize(&shell::strip_heredocs(text), ";&|\n`", true, true, " \t\r");
    search_words::segments(tokens)
        .into_iter()
        .find_map(|segment| inspect_segment(&segment.words, base, home, depth))
}

fn inspect_segment(words: &[String], base: &str, home: &str, depth: usize) -> Option<Hit> {
    if words.is_empty() || depth > MAX_NESTING {
        return None;
    }
    let is_tool = |name: &str| GREP_LIKE.contains(&name) || name == "find";
    let index = match search_words::locate(words, &["{", "}", "!"], is_tool) {
        Located::Nested(text) => return inspect_command(&text, base, home, depth + 1),
        Located::Tool(index) => index,
        Located::Other => return None,
    };
    let executable = shell::basename(&words[index]);
    let mut arguments = words[index + 1..].to_vec();
    let replacement = if executable == "find" {
        find::to_fd(&arguments)?
    } else {
        if executable == "rgrep" {
            arguments.insert(0, "-r".into());
        }
        grep::to_rg(&arguments, base, home)?
    };
    let mut original = vec![words[index].clone()];
    original.extend(arguments);
    Some(Hit {
        tool: words[index].clone(),
        original: render(&original),
        replacement: render(&replacement),
    })
}

fn exact_pattern_file_rewrite(text: &str, cwd: &str, home: &str) -> Option<String> {
    let spans = shell::argv_spans(text).ok()?;
    let chars: Vec<char> = text.chars().collect();
    let slice = |(start, end): (usize, usize)| chars[start..end].iter().collect::<String>();
    let explicit_file = |raw: &str| {
        if raw.contains(['$', '`', '*', '?', '[']) {
            return false;
        }
        match shell::shlex_split(raw).as_deref() {
            Some([word]) => {
                !word.starts_with('-')
                    && !search_words::is_dir(&search_words::join(
                        cwd,
                        &search_words::tilde(word, home),
                    ))
            }
            _ => false,
        }
    };
    let mut edits = Vec::new();
    let mut segment: Vec<(usize, usize)> = Vec::new();
    for (kind, start, end) in spans.into_iter().chain([("sep", chars.len(), chars.len())]) {
        if kind != "sep" {
            segment.push((start, end));
            continue;
        }
        if let [executable, option, patterns, path] = segment[..] {
            let patterns_text = slice(patterns);
            if slice(executable) == "grep"
                && slice(option) == "-qFf"
                && (patterns_text.starts_with("<(") || explicit_file(&patterns_text))
                && explicit_file(&slice(path))
            {
                edits.push((executable, "rg"));
                edits.push((option, "-qF -f"));
            }
        }
        segment.clear();
    }
    let mut result = chars;
    for ((start, end), replacement) in edits.into_iter().rev() {
        result.splice(start..end, replacement.chars());
    }
    Some(result.into_iter().collect())
}
