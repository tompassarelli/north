//! The implicit-state rule of code health: it finds state a program keeps outside a typed lifecycle (a
//! stamped marker file, a file read as a flag, a stopped unit holding an intent, a pid probed in /proc, a
//! name suffix read as a phase, a status compared as a string, lifecycle booleans) in Rust and TypeScript
//! source text. It reads text handed to it and a structure.toml's `[implicit_state]` table, never the file
//! system, and depends only on north-base's ast-grep runner.
mod classify;
mod config;
mod presence;
mod rules;
mod scan;
mod shape;
mod status;
mod structs;
#[cfg(test)]
mod tests;

pub use config::{Allow, Config};
pub use shape::{SHAPES, Shape};

use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub path: &'a str,
    pub text: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub shape: Shape,
    pub path: String,
    pub line: usize,
    pub item: String,
}

impl Finding {
    pub fn describe(&self, config: &Config) -> String {
        format!(
            "{}: {} — {}; keep it in {}, or record why with an [[implicit_state.allow]] entry (shape, path, item, reason) in structure.toml or a 'Code-health-allow: implicit-state {} — <reason>' trailer",
            self.shape.name(),
            self.item,
            self.shape.why(),
            config.typed,
            self.path
        )
    }

    fn key(&self) -> (Shape, &str, &str) {
        (self.shape, &self.path, &self.item)
    }
}

pub fn available() -> bool {
    north_base::platform::ast_grep::available()
}

/// Every finding in `files`, with what `context` (the repository's hand-written sources) writes and removes
/// deciding which presence checks read a flag; allowed findings are dropped.
pub fn scan(config: &Config, files: &[Source], context: &[Source]) -> Result<Vec<Finding>, String> {
    Ok(each(config, files, context)?.into_iter().flatten().filter(|finding| !allowed(config, finding)).collect())
}

/// The debt findings `head` adds over `base`: for each shape, path and item, every head occurrence when the
/// head has more of them than the base.
pub fn added(config: &Config, head: &[Source], base: &[Source], context: &[Source]) -> Result<Vec<Finding>, String> {
    let both: Vec<Source> = head.iter().chain(base).copied().collect();
    let mut found = each(config, &both, context)?;
    let base_found = found.split_off(head.len());
    let mut before: BTreeMap<(Shape, &str, &str), usize> = BTreeMap::new();
    for finding in base_found.iter().flatten() {
        *before.entry(finding.key()).or_default() += 1;
    }
    let head_found: Vec<Finding> = found.into_iter().flatten().filter(|finding| finding.shape.debt() && !allowed(config, finding)).collect();
    let mut after: BTreeMap<(Shape, &str, &str), usize> = BTreeMap::new();
    for finding in &head_found {
        *after.entry(finding.key()).or_default() += 1;
    }
    Ok(head_found.iter().filter(|finding| after[&finding.key()] > before.get(&finding.key()).copied().unwrap_or(0)).cloned().collect())
}

/// The repository's listing: a count per shape, each finding, and each allow entry that no longer covers one.
pub fn listing(config: &Config, files: &[Source]) -> Result<String, String> {
    let found: Vec<Finding> = each(config, files, files)?.into_iter().flatten().collect();
    let mut counts: BTreeMap<Shape, (usize, usize)> = BTreeMap::new();
    let mut lines = Vec::new();
    for finding in &found {
        let allowed = allowed(config, finding);
        let count = counts.entry(finding.shape).or_default();
        if allowed {
            count.1 += 1;
        } else {
            count.0 += 1;
            lines.push(format!("{}:{} {} {}", finding.path, finding.line, finding.shape.name(), finding.item));
        }
    }
    let mut out = vec![format!("implicit-state: {} files read", files.len())];
    out.extend(SHAPES.iter().map(|shape| {
        let (open, allowed) = counts.get(shape).copied().unwrap_or_default();
        format!("  {:<16} {open:>4} open {allowed:>3} allowed{}", shape.name(), if shape.debt() { "" } else { "  (listed, not debt)" })
    }));
    out.extend(lines);
    out.extend(config.allow.iter().filter(|allow| !found.iter().any(|finding| allow.covers(finding.shape, &finding.path, &finding.item))).map(|allow| format!("stale allow: {} {} {}", allow.shape.name(), allow.path, allow.item.as_deref().unwrap_or("*"))));
    Ok(out.join("\n"))
}

fn allowed(config: &Config, finding: &Finding) -> bool {
    config.allow.iter().any(|allow| allow.covers(finding.shape, &finding.path, &finding.item))
}

fn each(config: &Config, files: &[Source], context: &[Source]) -> Result<Vec<Vec<Finding>>, String> {
    let mut sources = files.to_vec();
    sources.extend(context.iter().filter(|source| !files.iter().any(|file| file.path == source.path) && presence::WRITERS.iter().any(|writer| source.text.contains(writer))));
    let matches = scan::matches(&sources)?;
    let world = presence::World::of(&sources, &matches, context);
    Ok(files.iter().zip(&matches).map(|(source, found)| classify::findings(config, source, found, &world)).collect())
}
