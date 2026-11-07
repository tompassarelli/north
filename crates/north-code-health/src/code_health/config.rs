use super::snapshot::{Snapshot, git_ok};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Thresholds {
    pub(super) file_lines: f64,
    pub(super) function_lines: f64,
    pub(super) function_complexity: f64,
    pub(super) hotspot_top: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Review {
    pub(super) commits: f64,
    pub(super) hand_lines_added: f64,
    pub(super) test_lines_changed: f64,
    pub(super) days: f64,
    pub(super) hotspot_score: f64,
    pub(super) hidden_links: f64,
    pub(super) volatile_imports: f64,
    pub(super) special_cases: f64,
}

pub(super) struct Module {
    pub(super) name: String,
    pub(super) may_use: Vec<String>,
    pub(super) facade: bool,
    globs: Vec<Regex>,
    pub(super) thresholds: Thresholds,
}

pub(super) struct Generated {
    pub(super) paths: Vec<String>,
    pub(super) producer: String,
    globs: Vec<Regex>,
}

pub(super) struct Config {
    pub(super) present: bool,
    pub(super) thresholds: Thresholds,
    pub(super) review: Review,
    pub(super) modules: Vec<Module>,
    pub(super) generated: Vec<Generated>,
    ignore: Vec<Regex>,
    safe_push_generated: HashSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Hand,
    Generated,
    Ignored,
    Other,
}

const DEFAULT_THRESHOLDS: Thresholds = Thresholds { file_lines: 500.0, function_lines: 80.0, function_complexity: 15.0, hotspot_top: 20.0 };
const DEFAULT_REVIEW: Review = Review { commits: 100.0, hand_lines_added: 5000.0, test_lines_changed: 3000.0, days: 7.0, hotspot_score: 0.0, hidden_links: 0.0, volatile_imports: 0.0, special_cases: 0.0 };

pub(super) fn glob(pattern: &str) -> Regex {
    let mut out = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                let slash_after = chars.get(index + 2) == Some(&'/');
                let at_segment = index == 0 || chars[index - 1] == '/';
                if at_segment && slash_after {
                    out.push_str("(?:.*/)?");
                    index += 3;
                } else {
                    out.push_str(".*");
                    index += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
        index += 1;
    }
    out.push('$');
    Regex::new(&out).unwrap_or_else(|_| Regex::new("^$").unwrap())
}

fn strings(item: Option<&toml_edit::Item>, place: &str) -> Result<Vec<String>, String> {
    let Some(item) = item else { return Ok(Vec::new()) };
    let error = || format!("structure.toml: {place} must be a list of strings");
    let array = item.as_array().ok_or_else(error)?;
    array.iter().map(|value| value.as_str().map(str::to_owned).ok_or_else(error)).collect()
}

fn table_entries(item: &toml_edit::Item) -> Option<Vec<(String, toml_edit::Item)>> {
    match item {
        toml_edit::Item::Table(table) => Some(table.iter().map(|(key, value)| (key.to_owned(), value.clone())).collect()),
        toml_edit::Item::Value(toml_edit::Value::InlineTable(table)) => Some(table.iter().map(|(key, value)| (key.to_owned(), toml_edit::Item::Value(value.clone()))).collect()),
        _ => None,
    }
}

fn numbers(fields: &mut [(&str, &mut f64)], item: Option<&toml_edit::Item>, place: &str) -> Result<(), String> {
    let Some(item) = item else { return Ok(()) };
    let entries = table_entries(item).ok_or_else(|| format!("structure.toml: [{place}] must be a table"))?;
    for (key, value) in entries {
        let slot = fields.iter_mut().find(|(name, _)| *name == key).ok_or_else(|| format!("structure.toml: unknown key {place}.{key}"))?;
        let number = value.as_float().or_else(|| value.as_integer().map(|n| n as f64)).filter(|n| *n >= 0.0);
        *slot.1 = number.ok_or_else(|| format!("structure.toml: {place}.{key} must be a non-negative number"))?;
    }
    Ok(())
}

fn thresholds(defaults: Thresholds, item: Option<&toml_edit::Item>, place: &str) -> Result<Thresholds, String> {
    let mut t = defaults;
    numbers(&mut [("file_lines", &mut t.file_lines), ("function_lines", &mut t.function_lines), ("function_complexity", &mut t.function_complexity), ("hotspot_top", &mut t.hotspot_top)], item, place)?;
    Ok(t)
}

fn review(item: Option<&toml_edit::Item>) -> Result<Review, String> {
    let mut r = DEFAULT_REVIEW;
    numbers(&mut [("commits", &mut r.commits), ("hand_lines_added", &mut r.hand_lines_added), ("test_lines_changed", &mut r.test_lines_changed), ("days", &mut r.days), ("hotspot_score", &mut r.hotspot_score), ("hidden_links", &mut r.hidden_links), ("volatile_imports", &mut r.volatile_imports), ("special_cases", &mut r.special_cases)], item, "review")?;
    Ok(r)
}

fn array_of_tables(document: &toml_edit::DocumentMut, key: &str) -> Vec<toml_edit::Table> {
    document.get(key).and_then(toml_edit::Item::as_array_of_tables).map(|tables| tables.iter().cloned().collect()).unwrap_or_default()
}

pub(super) fn parse(text: Option<&str>, safe_push_generated: HashSet<String>) -> Result<Config, String> {
    let document: toml_edit::DocumentMut = text.unwrap_or("").parse().map_err(|error: toml_edit::TomlError| format!("structure.toml: {error}"))?;
    let defaults = thresholds(DEFAULT_THRESHOLDS, document.get("thresholds"), "thresholds")?;
    let mut modules = Vec::new();
    for (index, table) in array_of_tables(&document, "module").iter().enumerate() {
        let name = table.get("name").and_then(toml_edit::Item::as_str).ok_or_else(|| format!("structure.toml: module {} has no name", index + 1))?.to_owned();
        let paths = strings(table.get("paths"), &format!("module {name}.paths"))?;
        modules.push(Module {
            thresholds: thresholds(defaults, table.get("thresholds"), &format!("module {name}.thresholds"))?,
            may_use: strings(table.get("may_use"), &format!("module {name}.may_use"))?,
            facade: table.get("facade").and_then(toml_edit::Item::as_bool).unwrap_or(false),
            globs: paths.iter().map(|path| glob(path)).collect(),
            name,
        });
    }
    check_modules(&modules)?;
    let mut generated = Vec::new();
    for (index, table) in array_of_tables(&document, "generated").iter().enumerate() {
        let producer = table.get("producer").and_then(toml_edit::Item::as_str).ok_or_else(|| format!("structure.toml: generated {} has no producer", index + 1))?.to_owned();
        let paths = strings(table.get("paths"), &format!("generated {}.paths", index + 1))?;
        generated.push(Generated { globs: paths.iter().map(|path| glob(path)).collect(), paths, producer });
    }
    let ignore = strings(document.get("ignore").and_then(|ignore| ignore.get("paths")), "ignore.paths")?.iter().map(|path| glob(path)).collect();
    Ok(Config { present: text.is_some(), thresholds: defaults, review: review(document.get("review"))?, modules, generated, ignore, safe_push_generated })
}

fn check_modules(modules: &[Module]) -> Result<(), String> {
    let names: HashSet<&str> = modules.iter().map(|module| module.name.as_str()).collect();
    if names.len() != modules.len() {
        return Err("structure.toml: duplicate module name".to_owned());
    }
    for module in modules {
        if let Some(unknown) = module.may_use.iter().find(|used| !names.contains(used.as_str())) {
            return Err(format!("structure.toml: module {} may_use unknown module {unknown}", module.name));
        }
    }
    let mut state: HashMap<&str, u8> = HashMap::new();
    fn visit<'a>(modules: &'a [Module], name: &'a str, path: &mut Vec<&'a str>, state: &mut HashMap<&'a str, u8>) -> Result<(), String> {
        match state.get(name) {
            Some(2) => return Ok(()),
            Some(1) => return Err(format!("structure.toml: may_use cycle {} -> {name}", path.join(" -> "))),
            _ => {}
        }
        state.insert(name, 1);
        path.push(name);
        let module = modules.iter().find(|module| module.name == name).expect("checked name");
        if !module.facade {
            for used in &module.may_use {
                visit(modules, used, path, state)?;
            }
        }
        path.pop();
        state.insert(name, 2);
        Ok(())
    }
    for module in modules {
        visit(modules, &module.name, &mut Vec::new(), &mut state)?;
    }
    Ok(())
}

fn safe_push_declarations(listing: Option<String>) -> HashSet<String> {
    let mut out = HashSet::new();
    for entry in listing.unwrap_or_default().split('\0') {
        let key = entry.split('\n').next().unwrap_or("");
        if let Some(path) = key.strip_prefix("generated.").and_then(|rest| rest.strip_suffix(".input")) {
            out.insert(path.to_owned());
        }
    }
    out
}

fn read_override(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn at_rev(snapshot: &Snapshot, override_path: Option<&Path>) -> Result<Config, String> {
    let listing = snapshot.has(".safe-push").then(|| git_ok(&snapshot.repo, &["config", "--blob", &format!("{}:.safe-push", snapshot.rev), "--null", "--list"])).flatten();
    let declarations = safe_push_declarations(listing);
    match override_path {
        Some(path) => parse(Some(&read_override(path)?), declarations),
        None => parse(snapshot.text("structure.toml").as_deref(), declarations),
    }
}

pub(super) fn in_worktree(repo: &Path, override_path: Option<&Path>) -> Result<Config, String> {
    let safe_push = repo.join(".safe-push");
    let listing = safe_push.exists().then(|| git_ok(repo, &["config", "--file", &safe_push.to_string_lossy(), "--null", "--list"])).flatten();
    let file = override_path.map_or_else(|| repo.join("structure.toml"), Path::to_path_buf);
    let text = if file.exists() { Some(read_override(&file)?) } else { None };
    parse(text.as_deref(), safe_push_declarations(listing))
}

pub(super) fn is_source(path: &str) -> bool {
    super::functions::language(path).is_some()
}

impl Config {
    pub(super) fn classify(&self, path: &str, exists: impl Fn(&str) -> bool) -> Kind {
        if !is_source(path) {
            return Kind::Other;
        }
        if self.ignore.iter().any(|glob| glob.is_match(path)) {
            return Kind::Ignored;
        }
        if self.safe_push_generated.contains(path) {
            return Kind::Generated;
        }
        if self.generated.iter().any(|g| g.globs.iter().any(|glob| glob.is_match(path)) && exists(&g.producer)) {
            return Kind::Generated;
        }
        Kind::Hand
    }

    pub(super) fn module_of(&self, path: &str) -> Option<&Module> {
        self.modules.iter().find(|module| module.globs.iter().any(|glob| glob.is_match(path)))
    }

    pub(super) fn thresholds_of(&self, path: &str) -> Thresholds {
        self.module_of(path).map_or(self.thresholds, |module| module.thresholds)
    }
}
