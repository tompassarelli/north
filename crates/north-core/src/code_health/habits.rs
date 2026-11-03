use super::rust_text::RustFile;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use toml_edit::{DocumentMut, Item};

pub(super) const SHARED: &str = "shared-mutability";
pub(super) const SINGLE: &str = "single-impl-trait";
const STALE: &str = "stale-allow";
const ALLOW: &str = "or add a [[rust_habits.allow]] entry in structure.toml naming this file, the rule and a reason (rust-development skill)";

pub(super) struct Habits {
    checks: BTreeSet<String>,
    allow: Vec<Allow>,
    ignore: Vec<Regex>,
    tests: Vec<Regex>,
}

struct Allow {
    rule: String,
    path: String,
    item: Option<String>,
}

#[derive(Debug, PartialEq)]
pub(super) struct Finding {
    pub(super) rule: &'static str,
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) item: String,
    pub(super) text: String,
}

impl Finding {
    pub(super) fn render(&self) -> String {
        format!("{} {}:{} {}", self.rule, self.path, self.line, self.text)
    }
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|array| array.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn glob(pattern: &str) -> Regex {
    let mut out = String::from("^");
    let mut rest = pattern;
    while let Some(next) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**/") {
            out.push_str("(?:.*/)?");
            rest = after;
            continue;
        }
        match next {
            '*' if rest.starts_with("**") => {
                out.push_str(".*");
                rest = &rest[2..];
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
        rest = &rest[next.len_utf8()..];
    }
    out.push('$');
    Regex::new(&out).unwrap_or_else(|_| Regex::new("$^").expect("empty pattern"))
}

impl Habits {
    pub(super) fn parse(text: &str) -> Result<Option<Self>, String> {
        let document = text.parse::<DocumentMut>().map_err(|error| format!("structure.toml: {error}"))?;
        let Some(table) = document.get("rust_habits") else {
            return Ok(None);
        };
        let checks: BTreeSet<String> = strings(table.get("checks")).into_iter().collect();
        if let Some(unknown) = checks.iter().find(|check| ![SHARED, SINGLE].contains(&check.as_str())) {
            return Err(format!("structure.toml: rust_habits.checks names unknown check {unknown}"));
        }
        let mut allow = Vec::new();
        for entry in table.get("allow").and_then(Item::as_array_of_tables).into_iter().flatten() {
            let field = |key: &str| entry.get(key).and_then(Item::as_str).map(str::to_owned);
            let (Some(rule), Some(path), Some(reason)) = (field("rule"), field("path"), field("reason")) else {
                return Err("structure.toml: each [[rust_habits.allow]] needs rule, path and reason".into());
            };
            if reason.trim().is_empty() || ![SHARED, SINGLE].contains(&rule.as_str()) {
                return Err(format!("structure.toml: allow entry for {path} needs a known rule and a reason"));
            }
            allow.push(Allow { rule, path, item: field("item") });
        }
        let ignore = strings(document.get("ignore").and_then(|ignore| ignore.get("paths")));
        let tests = document
            .get("module")
            .and_then(Item::as_array_of_tables)
            .into_iter()
            .flatten()
            .filter(|module| module.get("name").and_then(Item::as_str) == Some("tests"))
            .flat_map(|module| strings(module.get("paths")))
            .map(|pattern| glob(&pattern))
            .collect();
        Ok(Some(Self { checks, allow, ignore: ignore.iter().map(|pattern| glob(pattern)).collect(), tests }))
    }

    fn allowed(&self, finding: &Finding) -> bool {
        self.allow.iter().any(|entry| allows(entry, finding))
    }

    fn test_path(&self, path: &str) -> bool {
        let file = path.rsplit('/').next().unwrap_or(path);
        let parts: Vec<&str> = path.split('/').collect();
        self.tests.iter().any(|pattern| pattern.is_match(path))
            || file == "tests.rs"
            || file.ends_with("_tests.rs")
            || parts[..parts.len() - 1].iter().any(|part| matches!(*part, "tests" | "benches" | "examples"))
    }
}

fn allows(entry: &Allow, finding: &Finding) -> bool {
    entry.rule == finding.rule && entry.path == finding.path && entry.item.as_ref().is_none_or(|item| *item == finding.item)
}

pub(super) fn files_of(habits: &Habits, sources: Vec<(String, String)>) -> Vec<RustFile> {
    let mut files: Vec<RustFile> = sources
        .into_iter()
        .filter(|(path, _)| !habits.ignore.iter().any(|pattern| pattern.is_match(path)))
        .map(|(path, text)| {
            let mut file = RustFile::new(&path, &text);
            file.whole_test |= habits.test_path(&path);
            file
        })
        .collect();
    loop {
        let declared: BTreeSet<String> = files.iter().flat_map(|file| file.child_modules(false)).collect();
        let mut changed = false;
        for file in files.iter_mut().filter(|file| !file.whole_test && declared.contains(&file.path)) {
            file.whole_test = true;
            changed = true;
        }
        if !changed {
            return files;
        }
    }
}

fn shared_findings(files: &[RustFile]) -> Vec<Finding> {
    files
        .iter()
        .flat_map(|file| {
            file.shared_state().into_iter().map(|(line, kind)| Finding {
                rule: SHARED,
                path: file.path.clone(),
                line,
                text: format!("{kind} in non-test code — pass &mut down, split the borrow by field, index into a Vec or arena, or move ownership to the caller; {ALLOW}"),
                item: kind,
            })
        })
        .collect()
}

fn single_findings(files: &[RustFile]) -> Vec<Finding> {
    let mut impls: BTreeMap<String, (Vec<(String, usize)>, usize)> = BTreeMap::new();
    for file in files {
        for found in file.impls() {
            let entry = impls.entry(found.name).or_default();
            if found.test || file.whole_test {
                entry.1 += 1;
            } else {
                entry.0.push((file.path.clone(), found.line));
            }
        }
    }
    let mut out = Vec::new();
    for file in files.iter().filter(|file| !file.whole_test) {
        for (name, line) in file.traits() {
            let Some((real, 0)) = impls.get(&name) else { continue };
            let [(at, at_line)] = real.as_slice() else { continue };
            out.push(Finding {
                rule: SINGLE,
                path: file.path.clone(),
                line,
                item: name.clone(),
                text: format!("trait {name} has one implementation ({at}:{at_line}) and no test double — make the code concrete until a second implementation or a test seam uses it; {ALLOW} and item = \"{name}\""),
            });
        }
    }
    out
}

pub(super) fn check(habits: &Habits, files: &[RustFile], edited: Option<&str>) -> Vec<Finding> {
    let mut found = Vec::new();
    if habits.checks.contains(SHARED) {
        found.extend(shared_findings(files));
    }
    if habits.checks.contains(SINGLE) {
        found.extend(single_findings(files));
    }
    let mut out: Vec<Finding> = Vec::new();
    if edited.is_none() {
        for entry in habits.allow.iter().filter(|entry| !found.iter().any(|finding| allows(entry, finding))) {
            let item = entry.item.clone().unwrap_or_default();
            let text = format!("{} {item} matches nothing now — remove the entry from structure.toml", entry.rule);
            out.push(Finding { rule: STALE, path: entry.path.clone(), line: 0, item, text });
        }
    }
    let touches = |finding: &Finding| {
        edited.is_none_or(|path| finding.path == path || (finding.rule == SINGLE && finding.text.contains(&format!("({path}:"))))
    };
    out.extend(found.into_iter().filter(|finding| !habits.allowed(finding) && touches(finding)));
    out
}

fn listed(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.rs"])
        .output()
        .map_err(|error| format!("git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

pub fn findings(root: &Path, edited: Option<&str>) -> Result<Vec<String>, String> {
    let Ok(config) = std::fs::read_to_string(root.join("structure.toml")) else {
        return Ok(Vec::new());
    };
    let Some(habits) = Habits::parse(&config)? else {
        return Ok(Vec::new());
    };
    let sources = listed(root)?
        .into_iter()
        .filter_map(|path| std::fs::read_to_string(root.join(&path)).ok().map(|text| (path, text)))
        .collect();
    let files = files_of(&habits, sources);
    let mut lines: Vec<String> = check(&habits, &files, edited).iter().map(Finding::render).collect();
    lines.dedup();
    Ok(lines)
}
