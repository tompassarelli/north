use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

const SHARED: &str = "north_base";

static LOCAL_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(crate|super)::(\{[^}]*\}|[A-Za-z_][A-Za-z0-9_]*)").unwrap());
static NAMED_CRATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)::").unwrap());

pub(super) fn islands(files: &[(&str, &str)], workspace: &BTreeSet<String>, own: &str) -> Vec<Vec<String>> {
    let root = unit_root(files);
    let base = if root.is_empty() { String::new() } else { format!("{root}/") };
    let qualified = format!("crate::{}::", root.replace('/', "::"));
    let units: Vec<(&str, String)> = files.iter().filter_map(|(path, text)| path.strip_prefix(base.as_str()).map(|rest| (rest, text.replace(&qualified, "crate::")))).collect();
    let modules: Vec<&str> = units.iter().filter_map(|(path, _)| module_of(path)).collect::<BTreeSet<_>>().into_iter().collect();
    let position: BTreeMap<&str, usize> = modules.iter().enumerate().map(|(at, name)| (*name, at)).collect();
    let mut joins = Joins { parent: (0..modules.len()).collect(), reached: BTreeMap::new() };
    for (path, text) in &units {
        let Some(module) = module_of(path) else { continue };
        let here = position[module];
        joins.local(text, module, here, is_top_level(path), &position);
        joins.crates(text, here, workspace, own);
    }
    joins.groups(&modules)
}

fn unit_root(files: &[(&str, &str)]) -> String {
    let mut root = String::new();
    loop {
        let base = if root.is_empty() { String::new() } else { format!("{root}/") };
        let names: BTreeSet<&str> = files.iter().filter_map(|(path, _)| path.strip_prefix(base.as_str()).and_then(module_of)).collect();
        let [only] = names.into_iter().collect::<Vec<_>>()[..] else { return root };
        let dir = format!("{base}{only}/");
        if !files.iter().any(|(path, _)| path.starts_with(&dir)) {
            return root;
        }
        root = format!("{base}{only}");
    }
}

struct Joins {
    parent: Vec<usize>,
    reached: BTreeMap<String, usize>,
}

impl Joins {
    fn local(&mut self, text: &str, module: &str, here: usize, top: bool, position: &BTreeMap<&str, usize>) {
        for capture in LOCAL_PATH.captures_iter(text) {
            if &capture[1] == "super" && !top {
                continue;
            }
            for target in targets(&capture[2]).into_iter().filter(|target| *target != module) {
                if let Some(&there) = position.get(target) {
                    self.union(here, there);
                }
            }
        }
    }

    fn crates(&mut self, text: &str, here: usize, workspace: &BTreeSet<String>, own: &str) {
        for capture in NAMED_CRATE.captures_iter(text) {
            let name = capture.get(1).map_or("", |name| name.as_str());
            if workspace.contains(name) && name != own && name != SHARED {
                self.reach(name, here);
            }
        }
    }

    fn reach(&mut self, crate_name: &str, module: usize) {
        match self.reached.get(crate_name).copied() {
            Some(first) => self.union(first, module),
            None => {
                self.reached.insert(crate_name.to_owned(), module);
            }
        }
    }

    fn union(&mut self, left: usize, right: usize) {
        let (left, right) = (self.find(left), self.find(right));
        self.parent[right.max(left)] = right.min(left);
    }

    fn find(&mut self, mut at: usize) -> usize {
        while self.parent[at] != at {
            self.parent[at] = self.parent[self.parent[at]];
            at = self.parent[at];
        }
        at
    }

    fn groups(mut self, modules: &[&str]) -> Vec<Vec<String>> {
        let mut groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (at, name) in modules.iter().enumerate() {
            let root = self.find(at);
            groups.entry(root).or_default().push((*name).to_owned());
        }
        groups.into_values().collect()
    }
}

fn module_of(path: &str) -> Option<&str> {
    if matches!(path, "lib.rs" | "main.rs" | "mod.rs") {
        return None;
    }
    let first = path.split('/').next()?;
    Some(first.strip_suffix(".rs").unwrap_or(first))
}

fn is_top_level(path: &str) -> bool {
    match path.split('/').collect::<Vec<_>>().as_slice() {
        [_] => true,
        [_, file] => *file == "mod.rs",
        _ => false,
    }
}

fn targets(path: &str) -> Vec<&str> {
    match path.strip_prefix('{').and_then(|inner| inner.strip_suffix('}')) {
        Some(inner) => inner.split(',').filter_map(|piece| piece.trim().split("::").next()).filter(|name| !name.is_empty()).collect(),
        None => vec![path],
    }
}
