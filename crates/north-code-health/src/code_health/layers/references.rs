use super::super::imports::rust_references;
use super::super::rust_modules::Crate;
use super::super::source::Source;

pub(super) struct Reference {
    pub(super) line: usize,
    pub(super) target: String,
}

pub(super) fn references(path: &str, text: &str, source: &Source, crates: &[Crate]) -> Vec<Reference> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    rust_references(path, text, source, crates)
        .into_iter()
        .map(|(at, target)| Reference { line: text.as_bytes()[..at].iter().filter(|byte| **byte == b'\n').count() + 1, target })
        .collect()
}
