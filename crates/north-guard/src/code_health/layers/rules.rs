use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item, Table};

#[derive(Clone)]
struct Glob(Regex);

impl Glob {
    fn new(pattern: &str) -> Result<Self, String> {
        let mut out = String::from("^");
        let mut rest = pattern;
        while let Some(character) = rest.chars().next() {
            let (piece, width) = match character {
                '*' if rest.starts_with("**/") => ("(?:.*/)?".to_owned(), 3),
                '*' if rest.starts_with("**") => (".*".to_owned(), 2),
                '*' => ("[^/]*".to_owned(), 1),
                '?' => ("[^/]".to_owned(), 1),
                other => (regex::escape(other.encode_utf8(&mut [0; 4])), other.len_utf8()),
            };
            out.push_str(&piece);
            rest = &rest[width..];
        }
        out.push('$');
        Regex::new(&out).map(Self).map_err(|error| format!("structure.toml: glob {pattern}: {error}"))
    }

    fn matches(&self, path: &str) -> bool {
        self.0.is_match(path)
    }
}

#[derive(Clone)]
struct Group {
    name: String,
    globs: Vec<Glob>,
    may_use: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rule {
    Layer,
    CrateDag,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Breach {
    pub(super) rule: Rule,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) allowed: Vec<String>,
}

#[derive(Clone, Default)]
pub(super) struct Rules {
    ignore: Vec<Glob>,
    modules: Vec<Group>,
    crates: Vec<Group>,
    re_exports: Vec<Glob>,
    reach: BTreeMap<String, BTreeSet<String>>,
}

impl Rules {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| format!("structure.toml: {error}"))?;
        let ignore = match document.get("ignore").and_then(|item| item.get("paths")) {
            Some(paths) => globs(paths, "ignore.paths")?,
            None => Vec::new(),
        };
        let modules = groups(document.get("module"), "module", "paths")?;
        let (crates, re_exports) = match document.get("crate_dag") {
            Some(dag) => {
                let dag = dag.as_table().ok_or("structure.toml: crate_dag must be a table")?;
                (groups(dag.get("crate"), "crate_dag.crate", "paths")?, dag.get("re_exports").map_or(Ok(Vec::new()), |item| globs(item, "crate_dag.re_exports"))?)
            }
            None => (Vec::new(), Vec::new()),
        };
        let reach = closure(&crates)?;
        Ok(Self { ignore, modules, crates, re_exports, reach })
    }

    pub(super) fn or(mut self, fallback: &Self) -> Self {
        if self.modules.is_empty() {
            self.modules.clone_from(&fallback.modules);
        }
        if self.crates.is_empty() {
            self.crates.clone_from(&fallback.crates);
            self.re_exports.clone_from(&fallback.re_exports);
            self.reach.clone_from(&fallback.reach);
        }
        self
    }

    pub(super) fn checked(&self, path: &str) -> bool {
        path.ends_with(".rs") && !self.ignore.iter().any(|glob| glob.matches(path))
    }

    pub(super) fn breaches(&self, from: &str, to: &str) -> Vec<Breach> {
        if !self.checked(to) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let (Some(a), Some(b)) = (first(&self.modules, from), first(&self.modules, to))
            && a.name != b.name
            && !a.may_use.contains(&b.name)
        {
            out.push(Breach { rule: Rule::Layer, from: a.name.clone(), to: b.name.clone(), allowed: a.may_use.clone() });
        }
        let root = |path: &str| self.re_exports.iter().any(|glob| glob.matches(path));
        if let (false, false, Some(a), Some(b)) = (root(from), root(to), first(&self.crates, from), first(&self.crates, to))
            && a.name != b.name
            && !self.reach.get(&a.name).is_some_and(|reached| reached.contains(&b.name))
        {
            out.push(Breach { rule: Rule::CrateDag, from: a.name.clone(), to: b.name.clone(), allowed: a.may_use.clone() });
        }
        out
    }

    #[cfg(test)]
    pub(super) fn crate_plan(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.crates.iter().map(|krate| (krate.name.clone(), krate.may_use.iter().cloned().collect())).collect()
    }
}

fn first<'a>(groups: &'a [Group], path: &str) -> Option<&'a Group> {
    groups.iter().find(|group| group.globs.iter().any(|glob| glob.matches(path)))
}

fn strings(item: &Item, at: &str) -> Result<Vec<String>, String> {
    let array = item.as_array().ok_or_else(|| format!("structure.toml: {at} must be a list of strings"))?;
    array.iter().map(|value| value.as_str().map(str::to_owned).ok_or_else(|| format!("structure.toml: {at} must be a list of strings"))).collect()
}

fn globs(item: &Item, at: &str) -> Result<Vec<Glob>, String> {
    strings(item, at)?.iter().map(|pattern| Glob::new(pattern)).collect()
}

fn groups(item: Option<&Item>, at: &str, key: &str) -> Result<Vec<Group>, String> {
    let Some(item) = item else { return Ok(Vec::new()) };
    let tables = item.as_array_of_tables().ok_or_else(|| format!("structure.toml: {at} must be an array of tables"))?;
    let groups: Vec<Group> = tables.iter().enumerate().map(|(index, table)| group(table, &format!("{at} {}", index + 1), key)).collect::<Result<_, _>>()?;
    let names: BTreeSet<&str> = groups.iter().map(|group| group.name.as_str()).collect();
    if names.len() != groups.len() {
        return Err(format!("structure.toml: duplicate {at} name"));
    }
    if let Some((group, unknown)) = groups.iter().find_map(|group| group.may_use.iter().find(|name| !names.contains(name.as_str())).map(|name| (group, name))) {
        return Err(format!("structure.toml: {at} {} may_use unknown {unknown}", group.name));
    }
    Ok(groups)
}

fn group(table: &Table, at: &str, key: &str) -> Result<Group, String> {
    let name = table.get("name").and_then(Item::as_str).ok_or_else(|| format!("structure.toml: {at} has no name"))?.to_owned();
    let globs = table.get(key).map_or(Ok(Vec::new()), |item| globs(item, &format!("{at} {name}.{key}")))?;
    let may_use = table.get("may_use").map_or(Ok(Vec::new()), |item| strings(item, &format!("{at} {name}.may_use")))?;
    Ok(Group { name, globs, may_use })
}

fn closure(crates: &[Group]) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let edges: BTreeMap<&str, &[String]> = crates.iter().map(|krate| (krate.name.as_str(), krate.may_use.as_slice())).collect();
    let mut reach = BTreeMap::new();
    for krate in crates {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<String> = krate.may_use.clone();
        while let Some(next) = stack.pop() {
            if next == krate.name {
                return Err(format!("structure.toml: crate_dag cycle through {}", krate.name));
            }
            if seen.insert(next.clone()) {
                stack.extend(edges.get(next.as_str()).map_or(&[][..], |uses| uses).iter().cloned());
            }
        }
        reach.insert(krate.name.clone(), seen);
    }
    Ok(reach)
}
