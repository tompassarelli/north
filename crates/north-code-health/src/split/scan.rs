use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Sources {
    pub prod: String,
    pub tests: String,
    pub doctests: bool,
}

pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return if dir.extension().is_some_and(|extension| extension == "rs") && dir.is_file() { vec![dir.to_path_buf()] } else { files };
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

pub fn read(files: &[(String, String)]) -> Sources {
    let gated = gated_modules(files);
    let mut sources = Sources::default();
    for (path, text) in files {
        let text = code_only(&without_line_comments(text));
        sources.doctests |= has_doctest(text.as_str());
        if test_file(path, &gated) {
            sources.tests.push_str(&text);
            continue;
        }
        let (prod, tests) = split_inline_tests(&text);
        sources.prod.push_str(&prod);
        sources.tests.push_str(&tests);
    }
    sources
}

fn test_file(path: &str, gated: &BTreeSet<String>) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    file == "tests.rs" || file.ends_with("_tests.rs") || gated.iter().any(|module| path == format!("{module}.rs") || path.starts_with(&format!("{module}/")))
}

fn gated_modules(files: &[(String, String)]) -> BTreeSet<String> {
    let mut gated = BTreeSet::new();
    for (path, text) in files {
        let dir = module_dir(path);
        let mut previous = "";
        for line in text.lines() {
            if previous.trim() == "#[cfg(test)]"
                && let Some(name) = declared_module(line)
            {
                gated.insert(format!("{dir}/{name}"));
            }
            previous = line;
        }
    }
    gated
}

fn module_dir(path: &str) -> String {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    match file {
        "mod.rs" | "lib.rs" | "main.rs" => dir.to_string(),
        _ => format!("{dir}/{}", file.trim_end_matches(".rs")),
    }
}

pub fn declared_module(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix("pub ").or_else(|| line.split_once(") ").filter(|(head, _)| head.starts_with("pub(")).map(|(_, rest)| rest)).unwrap_or(line);
    let name = rest.strip_prefix("mod ")?.strip_suffix(';')?.trim();
    name.chars().all(|c| c.is_alphanumeric() || c == '_').then_some(name)
}

fn split_inline_tests(text: &str) -> (String, String) {
    let (mut prod, mut tests) = (String::new(), String::new());
    let mut in_tests = false;
    let mut previous = "";
    for line in text.lines() {
        let opens = line.trim_start().starts_with("mod ") && line.trim_end().ends_with('{');
        in_tests |= previous.trim() == "#[cfg(test)]" && opens;
        let target = if in_tests { &mut tests } else { &mut prod };
        target.push_str(line);
        target.push('\n');
        previous = line;
    }
    (prod, tests)
}

fn without_line_comments(text: &str) -> String {
    text.lines().filter(|line| !line.trim_start().starts_with("//") || is_doc(line)).map(|line| format!("{line}\n")).collect()
}

fn is_doc(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("///") || line.starts_with("//!")
}

fn has_doctest(text: &str) -> bool {
    text.lines().any(|line| is_doc(line) && line.contains("```"))
}

pub fn paths_after(text: &str, prefix: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let needle = format!("{prefix}::");
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let start = from + found;
        from = start + needle.len();
        if text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') {
            continue;
        }
        let mut chars = text[from..].chars().peekable();
        tree(&mut chars, String::new(), &mut paths);
    }
    paths
}

fn tree(chars: &mut std::iter::Peekable<std::str::Chars>, prefix: String, paths: &mut Vec<String>) {
    skip_space(chars);
    if chars.peek() == Some(&'{') {
        chars.next();
        loop {
            tree(chars, prefix.clone(), paths);
            skip_space(chars);
            match chars.next() {
                Some(',') => continue,
                _ => return,
            }
        }
    }
    let segment = ident(chars);
    if segment.is_empty() {
        if chars.peek() != Some(&'}') && !prefix.is_empty() {
            paths.push(prefix);
        }
        return;
    }
    let path = if prefix.is_empty() { segment } else { format!("{prefix}::{segment}") };
    let mut lookahead = chars.clone();
    if lookahead.next() == Some(':') && lookahead.next() == Some(':') {
        chars.next();
        chars.next();
        return tree(chars, path, paths);
    }
    skip_alias(chars);
    paths.push(path);
}

fn skip_alias(chars: &mut std::iter::Peekable<std::str::Chars>) {
    let mut lookahead = chars.clone();
    skip_space(&mut lookahead);
    if ident(&mut lookahead) == "as" {
        skip_space(&mut lookahead);
        ident(&mut lookahead);
        *chars = lookahead;
    }
}

fn skip_space(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

fn ident(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut word = String::new();
    while let Some(&c) = chars.peek().filter(|c| c.is_alphanumeric() || **c == '_' || **c == '*') {
        word.push(c);
        chars.next();
    }
    word
}

pub fn names_crate(text: &str, lib: &str) -> bool {
    let mut from = 0;
    while let Some(found) = text[from..].find(lib) {
        let start = from + found;
        let end = start + lib.len();
        from = end;
        let before = text[..start].chars().next_back();
        let boundary_before = !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':');
        let after = &text[end..];
        let path = after.starts_with("::") || after.starts_with('!');
        let used = text[..start].trim_end().ends_with("use") && !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        if boundary_before && (path || used) {
            return true;
        }
    }
    false
}

pub fn code_only(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '\'' && at + 2 < chars.len() && (chars[at + 2] == '\'' || (chars[at + 1] == '\\' && at + 3 < chars.len() && chars[at + 3] == '\'')) {
            let end = if chars[at + 2] == '\'' { at + 3 } else { at + 4 };
            out.extend(std::iter::repeat_n(' ', end - at));
            at = end;
            continue;
        }
        let raw = c == 'r' && chars.get(at + 1).is_some_and(|next| *next == '"' || *next == '#') && !chars[..at].last().is_some_and(|before| before.is_alphanumeric() || *before == '_');
        if c == '"' || raw {
            let end = literal_end(&chars, at);
            out.extend(chars[at..end].iter().map(|c| if *c == '\n' { '\n' } else { ' ' }));
            at = end;
            continue;
        }
        out.push(c);
        at += 1;
    }
    out
}

fn literal_end(chars: &[char], start: usize) -> usize {
    let mut at = start;
    let mut hashes = 0;
    if chars[at] == 'r' {
        at += 1;
        while chars.get(at) == Some(&'#') {
            hashes += 1;
            at += 1;
        }
        if chars.get(at) != Some(&'"') {
            return start + 1;
        }
    }
    at += 1;
    while at < chars.len() {
        match chars[at] {
            '\\' if chars[start] != 'r' => at += 2,
            '"' if chars[at + 1..].iter().take(hashes).filter(|c| **c == '#').count() == hashes => return (at + 1 + hashes).min(chars.len()),
            _ => at += 1,
        }
    }
    chars.len()
}
