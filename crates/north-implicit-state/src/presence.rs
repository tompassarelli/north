use crate::Source;
use north_base::platform::ast_grep::Match;
use regex::Regex;
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

static CONST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]*)"\s*;"#).expect("const pattern"));
static LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:&\s*)?(?:mut\s+)?([a-z_][a-z0-9_]*)(\(\))?").expect("lead pattern"));
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""((?:[^"\\]|\\.)*)"|\b([A-Z][A-Z0-9_]+)\b"#).expect("token pattern"));
static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\.(?:as_path|as_ref|clone|to_path_buf)\(\)|\?)$").expect("tail pattern"));

pub(crate) const WRITERS: [&str; 4] = ["fs::write", "remove_file", "File::create", "create(true)"];

/// What the scanned repository writes and removes: each state path's anchor, the last literal component it
/// is built from, with `/*` when a computed name is joined after it.
#[derive(Default)]
pub(crate) struct World {
    consts: HashMap<String, Vec<(String, String)>>,
    writes: BTreeSet<String>,
    removes: BTreeSet<String>,
}

impl World {
    pub(crate) fn of(sources: &[Source], matches: &[Vec<Match>], named: &[Source]) -> Self {
        let mut world = Self::default();
        for source in sources.iter().chain(named.iter().filter(|named| !sources.iter().any(|source| source.path == named.path))) {
            for found in CONST.captures_iter(source.text) {
                world.consts.entry(found[1].to_owned()).or_default().push((source.path.to_owned(), found[2].to_owned()));
            }
        }
        for (source, found) in sources.iter().zip(matches) {
            for writer in found.iter().filter(|found| matches!(found.rule.as_str(), "state-write" | "state-open")) {
                let Some(anchor) = writer.single.get("P").and_then(|path| world.anchor(path, source, writer.start)) else { continue };
                if writer.text.contains("remove_file") {
                    world.removes.insert(anchor);
                } else {
                    world.writes.insert(anchor);
                }
            }
        }
        world
    }

    /// The anchor a presence check reads as a flag: a path this repository removes, or one of a directory of
    /// marks it writes under computed names. A file only ever written is created once, not toggled.
    pub(crate) fn flag(&self, source: &Source, found: &Match) -> Option<String> {
        let path = found.single.get("P").filter(|path| !path.contains("\"/proc"))?;
        let anchor = self.anchor(path, source, found.start)?;
        let marks = found.rule == "presence-check" && anchor.ends_with("/*") && self.writes.contains(&anchor);
        (self.removes.contains(&anchor) || marks).then_some(anchor)
    }

    fn anchor(&self, path: &str, source: &Source, at: usize) -> Option<String> {
        let expanded = expand(clean(path), source.text, at);
        let mut last = None;
        for found in TOKEN.captures_iter(&expanded) {
            let literal = match (found.get(1), found.get(2)) {
                (Some(literal), _) => Some(literal.as_str().to_owned()),
                (None, Some(name)) => match self.constant(name.as_str(), source) {
                    Some(value) => Some(value),
                    None if last.is_some() => return None,
                    None => continue,
                },
                _ => None,
            };
            if let Some(literal) = literal.filter(|literal| !literal.contains('{') && !literal.contains(' ')) {
                let component = literal.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
                if !component.is_empty() {
                    last = Some((component.to_owned(), found.get(0).map_or(0, |whole| whole.end())));
                }
            }
        }
        let (component, end) = last?;
        Some(if expanded[end..].contains(".join(") { format!("{component}/*") } else { component })
    }

    /// The value of the `&str` constant `name` as `source` sees it: its own, else the one its `use` names by
    /// module, else the only one in the repository.
    fn constant(&self, name: &str, source: &Source) -> Option<String> {
        let candidates = self.consts.get(name)?;
        if let Some((_, own)) = candidates.iter().find(|(path, _)| path == source.path) {
            return Some(own.clone());
        }
        let imported = Regex::new(&format!(r"use\s+([A-Za-z0-9_:]+)::(?:\{{[^}}]*\b{name}\b[^}}]*\}}|{name}\b)")).ok()?;
        let module = imported.captures(source.text).and_then(|found| found[1].rsplit("::").next().map(str::to_owned));
        let mut matching = candidates.iter().filter(|(path, _)| module.as_deref().is_none_or(|module| path.trim_end_matches(".rs").ends_with(module) || path.contains(&format!("/{module}/"))));
        match (matching.next(), matching.next()) {
            (Some((_, value)), None) => Some(value.clone()),
            _ => None,
        }
    }
}

fn clean(path: &str) -> &str {
    let mut path = path.trim().trim_start_matches('&').trim();
    while let Some(found) = TAIL.find(path) {
        path = path[..found.start()].trim_end();
    }
    path
}

/// The path expression with its leading local or zero-argument function replaced by its definition, a few
/// levels deep; a shadowing `let` resolves through the binding before it.
fn expand(path: &str, text: &str, at: usize) -> String {
    let mut expression = path.to_owned();
    let mut at = at.min(text.len());
    for _ in 0..6 {
        let Some(lead) = LEAD.captures(&expression) else { break };
        let (name, call, end) = (lead[1].to_owned(), lead.get(2).is_some(), lead.get(0).map_or(0, |whole| whole.end()));
        let definition = if call { body(&name, text) } else { binding(&name, &text[..at]) };
        let Some((definition, start)) = definition else { break };
        at = start;
        expression = format!("{}{}", clean(&definition), &expression[end..]);
    }
    expression
}

fn binding(name: &str, before: &str) -> Option<(String, usize)> {
    let pattern = Regex::new(&format!(r"(?s)\blet\s+(?:mut\s+)?{name}\s*(?::[^=;]+)?=\s*(.+?);")).ok()?;
    pattern.captures_iter(before).last().map(|found| (found[1].to_owned(), found.get(0).map_or(0, |whole| whole.start())))
}

fn body(name: &str, text: &str) -> Option<(String, usize)> {
    let pattern = Regex::new(&format!(r"\bfn\s+{name}\s*\(\s*\)[^{{;]*\{{")).ok()?;
    let open = pattern.find(text)?.end();
    let mut depth = 1;
    let close = text[open..].char_indices().find(|(_, at)| {
        match at {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        depth == 0
    })?;
    Some((text[open..open + close.0].trim().trim_end_matches(';').to_owned(), open + close.0))
}
