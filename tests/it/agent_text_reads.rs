use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const ALLOWLIST: &str = "harness/agent-text-reads.toml";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn is_test_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "tests.rs" || name.ends_with("_tests.rs") || path.split('/').any(|part| part == "tests")
}

static READ_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)has_opener\(|report_column\(|starts_with\("Stop:|\br#?"[^"\n]*(stop\s*:|stop\\s|stop\\W|not done|blocked:)"#).unwrap());

static LITERAL_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\br#*""#).unwrap());

fn reads(text: &str, all_patterns: bool) -> usize {
    let (read, literal) = (&*READ_PATTERN, &*LITERAL_PATTERN);
    let code = text.split("#[cfg(test)]").next().unwrap_or(text);
    code.lines().filter(|line| !line.trim_start().starts_with("//")).filter(|line| read.is_match(line) || (all_patterns && literal.is_match(line))).count()
}

fn sources(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path.strip_prefix(root()).unwrap().to_string_lossy().into_owned());
        }
    }
}

struct Entry {
    lines: usize,
    all_patterns: bool,
}

fn allowed() -> BTreeMap<String, Entry> {
    let doc: toml_edit::DocumentMut = fs::read_to_string(root().join(ALLOWLIST)).unwrap().parse().unwrap();
    doc["read"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .map(|table| {
            let entry = Entry { lines: table["lines"].as_integer().unwrap() as usize, all_patterns: table.get("all_patterns").and_then(|item| item.as_bool()).unwrap_or(false) };
            (table["file"].as_str().unwrap().to_owned(), entry)
        })
        .collect()
}

fn refusals(files: &BTreeMap<String, String>, allowed: &BTreeMap<String, Entry>) -> Vec<String> {
    let mut refused = Vec::new();
    for (path, text) in files.iter().filter(|(path, _)| !is_test_file(path)) {
        let listed = allowed.get(path);
        let found = reads(text, listed.is_some_and(|entry| entry.all_patterns));
        let expected = listed.map_or(0, |entry| entry.lines);
        if found > expected {
            refused.push(format!("{path}: {found} lines read agent text, {ALLOWLIST} allows {expected}; derive the verdict from a mechanical source (docs/rationale/attribution.md) instead"));
        } else if found < expected {
            refused.push(format!("{path}: {found} lines read agent text, {ALLOWLIST} still lists {expected}; lower or remove the entry"));
        }
    }
    for path in allowed.keys().filter(|path| !files.contains_key(*path)) {
        refused.push(format!("{path}: listed in {ALLOWLIST} but not a source file; remove the entry"));
    }
    refused
}

fn workspace() -> BTreeMap<String, String> {
    let mut found = Vec::new();
    sources(&root().join("src"), &mut found);
    sources(&root().join("crates"), &mut found);
    found.into_iter().filter(|path| path.contains("/src/") || path.starts_with("src/")).map(|path| (path.clone(), fs::read_to_string(root().join(&path)).unwrap_or_default())).collect()
}

#[test]
fn scorer_attribution_and_routing_read_no_agent_text_beyond_the_listed_inputs() {
    let refused = refusals(&workspace(), &allowed());
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

#[test]
fn a_planted_report_read_in_a_new_file_is_refused() {
    let mut files = workspace();
    files.insert("crates/north-pace/src/router/planted.rs".into(), "static CAPACITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"(?im)^\\W*Stop:\\s*capacity\\b\").unwrap());\n".into());
    let refused = refusals(&files, &allowed());
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].starts_with("crates/north-pace/src/router/planted.rs"));
}

#[test]
fn a_planted_report_read_in_a_listed_file_is_refused() {
    let mut files = workspace();
    let path = "crates/north-providers/src/providers/claude/runs/stop.rs";
    let text = files.get_mut(path).unwrap();
    text.insert_str(0, "fn planted(report: &str) -> bool { report.lines().any(|line| line.starts_with(\"Stop:\")) }\n");
    let refused = refusals(&files, &allowed());
    assert!(refused.iter().any(|line| line.starts_with(path) && line.contains("allows")), "{refused:?}");
}
