use crate::Source;
use crate::rules::{self, Language};
use north_base::platform::ast_grep::{self, File, Match};
use std::collections::BTreeMap;

/// Every rule's matches in each source, in the sources' order; one ast-grep run per language present.
pub(crate) fn matches(sources: &[Source]) -> Result<Vec<Vec<Match>>, String> {
    let mut out = vec![Vec::new(); sources.len()];
    let mut by_language: BTreeMap<Language, Vec<(usize, String)>> = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        if let Some(language) = rules::language(source.path).filter(|_| !test_file(source)) {
            by_language.entry(language).or_default().push((index, format!("{index}/{}", source.path)));
        }
    }
    for (language, entries) in by_language {
        let files: Vec<File> = entries.iter().map(|(index, path)| File { path, text: sources[*index].text }).collect();
        for found in ast_grep::scan(&files, &rules::rules(language))? {
            let Some(index) = found.file.split_once('/').and_then(|(index, _)| index.parse::<usize>().ok()) else { continue };
            if let Some(slot) = out.get_mut(index) {
                slot.push(found);
            }
        }
    }
    for found in &mut out {
        let scopes: Vec<(usize, usize)> = found.iter().filter(|found| found.rule == "test-scope").map(|found| (found.start, found.end)).collect();
        found.retain(|found| found.rule != "test-scope" && !scopes.iter().any(|(start, end)| *start <= found.start && found.end <= *end));
    }
    Ok(out)
}

/// A test source: a tests module file, a test or example directory, a spec, or a file compiled only for tests.
pub(crate) fn test_file(source: &Source) -> bool {
    let name = source.path.rsplit('/').next().unwrap_or(source.path);
    let directory = source.path.split('/').rev().skip(1).any(|part| matches!(part, "tests" | "benches" | "examples" | "__tests__" | "fixtures"));
    directory || name == "tests.rs" || name.ends_with("_tests.rs") || name.contains(".test.") || name.contains(".spec.") || source.text.trim_start().starts_with("#![cfg(test)]")
}
