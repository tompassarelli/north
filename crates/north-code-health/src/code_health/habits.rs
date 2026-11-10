use super::ambient::{self, Ambient};
use super::finding::Finding;
use super::fix::{self, Rewrite};
use super::rust_text::RustFile;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
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
    ambient: Vec<Ambient>,
}

struct Allow {
    rule: String,
    path: String,
    item: Option<String>,
}

fn ambient_config(table: &Item, checks: &BTreeSet<String>) -> Result<Vec<Ambient>, String> {
    let configs: Vec<&toml_edit::Table> = table.get("ambient_inputs").and_then(Item::as_array_of_tables).into_iter().flatten().collect();
    if configs.is_empty() && checks.contains(ambient::RULE) {
        return Err("structure.toml: rust_habits.checks names ambient-test-input without a [[rust_habits.ambient_inputs]] table".into());
    }
    configs
        .into_iter()
        .map(|config| {
            let text = |key: &str| config.get(key).and_then(Item::as_str).map(str::to_owned).ok_or_else(|| format!("structure.toml: each [[rust_habits.ambient_inputs]] needs {key}"));
            let name = text("input")?;
            let input = ambient::Input::named(&name).ok_or_else(|| format!("structure.toml: rust_habits.ambient_inputs names unknown input {name}; known: host, clock"))?;
            Ok(Ambient { input, fixtures: strings(config.get("fixtures")), allowlist: text("allowlist")?, fixture: text("fixture")?, allowed: Default::default() })
        })
        .collect()
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
        if let Some(unknown) = checks.iter().find(|check| ![SHARED, SINGLE, ambient::RULE].contains(&check.as_str())) {
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
        let ambient = ambient_config(table, &checks)?;
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
        Ok(Some(Self { checks, allow, ignore: ignore.iter().map(|pattern| glob(pattern)).collect(), tests, ambient }))
    }

    pub(super) fn read_ambient(&mut self, read: impl Fn(&str) -> Result<String, String>) -> Result<(), String> {
        self.ambient.iter_mut().try_for_each(|ambient| ambient.read_allowed(&read(&ambient.allowlist)?))
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
                fix: None,
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
                fix: None,
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
    if habits.checks.contains(ambient::RULE) {
        found.extend(habits.ambient.iter().flat_map(|ambient| ambient.findings(files, edited.is_none())));
    }
    let mut out: Vec<Finding> = Vec::new();
    if edited.is_none() {
        for entry in habits.allow.iter().filter(|entry| !found.iter().any(|finding| allows(entry, finding))) {
            let item = entry.item.clone().unwrap_or_default();
            let text = format!("{} {item} matches nothing now — remove the entry from structure.toml", entry.rule);
            out.push(Finding { rule: STALE, path: entry.path.clone(), line: 0, item, text, fix: None });
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

fn found(root: &Path, edited: Option<&str>) -> Result<Vec<Finding>, String> {
    let Ok(config) = std::fs::read_to_string(root.join("structure.toml")) else {
        return Ok(Vec::new());
    };
    let Some(mut habits) = Habits::parse(&config)? else {
        return Ok(Vec::new());
    };
    habits.read_ambient(|allowlist| std::fs::read_to_string(root.join(allowlist)).map_err(|error| format!("{allowlist}: {error}")))?;
    let sources = listed(root)?
        .into_iter()
        .filter_map(|path| std::fs::read_to_string(root.join(&path)).ok().map(|text| (path, text)))
        .collect();
    let files = files_of(&habits, sources);
    Ok(check(&habits, &files, edited))
}

pub fn findings(root: &Path, edited: Option<&str>) -> Result<Vec<String>, String> {
    let mut lines: Vec<String> = found(root, edited)?.iter().map(Finding::render).collect();
    lines.dedup();
    Ok(lines)
}

fn at_rev(root: &Path, rev: &str, path: &str) -> Result<String, String> {
    let verify = Command::new("git").arg("-C").arg(root).args(["rev-parse", "--verify", "--quiet", rev]).output().map_err(|error| format!("git rev-parse: {error}"))?;
    if !verify.status.success() {
        return Err(format!("git rev-parse: {rev} is not a commit"));
    }
    let output = Command::new("git").arg("-C").arg(root).args(["show", &format!("{rev}:{path}")]).output().map_err(|error| format!("git show: {error}"))?;
    Ok(if output.status.success() { String::from_utf8_lossy(&output.stdout).into_owned() } else { String::new() })
}

pub fn grown(root: &Path, rev: &str) -> Result<Vec<String>, String> {
    let Ok(config) = std::fs::read_to_string(root.join("structure.toml")) else {
        return Ok(Vec::new());
    };
    let Some(mut habits) = Habits::parse(&config)? else {
        return Ok(Vec::new());
    };
    habits.read_ambient(|allowlist| std::fs::read_to_string(root.join(allowlist)).map_err(|error| format!("{allowlist}: {error}")))?;
    let mut out = Vec::new();
    for ambient in &habits.ambient {
        let before = ambient::counts(&ambient.allowlist, &at_rev(root, rev, &ambient.allowlist)?)?;
        for (path, count) in &ambient.allowed {
            let was = before.get(path).copied().unwrap_or(0);
            if *count > was {
                out.push(format!("{} lists {count} ambient test reads in {path}, up from {was} at {rev} — the list only shrinks", ambient.allowlist));
            }
        }
    }
    Ok(out)
}

fn sources_at(root: &Path, rev: &str) -> Result<Vec<(String, String)>, String> {
    let output = Command::new("git").arg("-C").arg(root).args(["ls-tree", "-r", "-z", "--name-only", rev]).output().map_err(|error| format!("git ls-tree: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-tree: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let names: Vec<String> = String::from_utf8_lossy(&output.stdout).split('\0').filter(|name| name.ends_with(".rs")).map(str::to_owned).collect();
    let mut child = Command::new("git").arg("-C").arg(root).args(["cat-file", "--batch"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|error| format!("git cat-file: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file: no input")?;
    let request: String = names.iter().map(|name| format!("{rev}:{name}\n")).collect();
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()));
    let stdout = child.stdout.take().ok_or("git cat-file: no output")?;
    let mut reader = BufReader::new(stdout);
    let mut sources = Vec::with_capacity(names.len());
    for name in names {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|error| format!("git cat-file: {error}"))?;
        let Some(size) = header.split_whitespace().nth(2).and_then(|size| size.parse::<usize>().ok()) else { continue };
        let mut body = vec![0u8; size];
        reader.read_exact(&mut body).map_err(|error| format!("git cat-file: {error}"))?;
        let mut end = [0u8; 1];
        reader.read_exact(&mut end).map_err(|error| format!("git cat-file: {error}"))?;
        sources.push((name, String::from_utf8_lossy(&body).into_owned()));
    }
    let _ = writer.join();
    let _ = child.wait();
    Ok(sources)
}

fn found_at(root: &Path, rev: &str) -> Result<Vec<Finding>, String> {
    let Ok(config) = std::fs::read_to_string(root.join("structure.toml")) else {
        return Ok(Vec::new());
    };
    let Some(mut habits) = Habits::parse(&config)? else {
        return Ok(Vec::new());
    };
    habits.read_ambient(|allowlist| at_rev(root, rev, allowlist))?;
    let files = files_of(&habits, sources_at(root, rev)?);
    Ok(check(&habits, &files, None))
}

pub fn findings_new(root: &Path, rev: &str) -> Result<Vec<String>, String> {
    let mut known: BTreeMap<(&'static str, String, String), usize> = BTreeMap::new();
    for finding in found_at(root, rev)? {
        *known.entry((finding.rule, finding.path, finding.item)).or_default() += 1;
    }
    let mut out = Vec::new();
    for finding in found(root, None)? {
        match known.get_mut(&(finding.rule, finding.path.clone(), finding.item.clone())) {
            Some(left) if *left > 0 => *left -= 1,
            _ => out.push(finding.render()),
        }
    }
    out.extend(grown(root, rev)?);
    Ok(out)
}

pub(super) fn fix_file(root: &Path, edited: &str) -> Result<Vec<Rewrite>, String> {
    let findings = found(root, Some(edited))?;
    if findings.iter().all(|finding| finding.fix.is_none()) {
        return Ok(Vec::new());
    }
    let path = root.join(edited);
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{edited}: {error}"))?;
    let (fixed, rewrites) = fix::apply(&text, &findings);
    if !rewrites.is_empty() {
        std::fs::write(&path, fixed).map_err(|error| format!("{edited}: {error}"))?;
    }
    Ok(rewrites)
}
