use std::collections::{BTreeMap, BTreeSet};

use super::super::TestBinary;
use super::BINARY_MARK;

pub(super) fn binaries(printed: &str) -> Result<Vec<TestBinary>, String> {
    let mut sections: Vec<(String, BTreeSet<String>)> = Vec::new();
    for line in printed.lines() {
        if let Some(id) = line.strip_prefix(BINARY_MARK) {
            sections.push((id.trim().to_owned(), BTreeSet::new()));
        } else if let Some(name) = line.strip_suffix(": test") {
            sections.last_mut().ok_or_else(|| format!("the listing names the test {name} before any test binary"))?.1.insert(name.to_owned());
        }
    }
    let mut merged: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (id, tests) in sections {
        merged.entry(id).or_default().extend(tests);
    }
    Ok(merged.into_iter().map(|(id, tests)| TestBinary { package: id.split("::").next().unwrap_or_default().to_owned(), id, tests, ..TestBinary::default() }).collect())
}
