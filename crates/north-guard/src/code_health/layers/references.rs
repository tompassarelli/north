use super::super::imports::RS_USE;
use super::super::rust_modules::{Crate, expand_use, inline_modules, resolve_rust};
use super::super::rust_text::mask;
use super::super::source::Source;
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z0-9_:])((?:crate|super|self|[a-z][a-z0-9_]*)(?:\s*::\s*[A-Za-z_][A-Za-z0-9_]*)+)").unwrap()
});

pub(super) struct Reference {
    pub(super) line: usize,
    pub(super) target: String,
}

pub(super) fn references(path: &str, text: &str, source: &Source, crates: &[Crate]) -> Vec<Reference> {
    let code = mask(text.strip_prefix('\u{feff}').unwrap_or(text));
    let mut found = Vec::new();
    let mut statements: Vec<Range<usize>> = Vec::new();
    for statement in RS_USE.captures_iter(&code) {
        let whole = statement.get(0).expect("whole match");
        statements.push(whole.range());
        let inline = inline_modules(&code, whole.start());
        for segments in expand_use(&statement[1]) {
            found.extend(resolve_rust(path, &segments, source, crates, &inline).map(|target| (whole.start(), target)));
        }
    }
    for reference in PATH.captures_iter(&code) {
        let span = reference.get(1).expect("path group");
        if statements.iter().any(|statement| statement.contains(&span.start())) {
            continue;
        }
        let segments: Vec<String> = span.as_str().split("::").map(|part| part.trim().to_owned()).collect();
        let relative = matches!(segments[0].as_str(), "self" | "super");
        let inline = if relative { inline_modules(&code, span.start()) } else { Vec::new() };
        found.extend(resolve_rust(path, &segments, source, crates, &inline).map(|target| (span.start(), target)));
    }
    found.sort();
    found
        .into_iter()
        .filter(|(_, target)| target != path)
        .map(|(at, target)| Reference { line: code.as_bytes()[..at].iter().filter(|byte| **byte == b'\n').count() + 1, target })
        .collect()
}
