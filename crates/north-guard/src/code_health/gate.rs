use super::config::{Config, Kind, Thresholds, at_rev};
use super::functions::{Function, SourceText, measure};
use super::graph::{component_of, layer_violation, sccs};
use super::history::{allows, churn_at};
use super::report::{hand_of, hotspots_of, import_graph, inventory};
use super::resolver::Resolver;
use super::snapshot::{Snapshot, git, line_count};
use super::state::State;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) const FIX_CYCLE: &str = "move the import behind an interface the other file already owns, or merge the two files";
pub(super) const FIX_LAYER: &str = "move the import behind the allowed module's interface, or change may_use/paths in structure.toml in this change";
const FIX_FILE: &str = "split the file, or raise thresholds.file_lines in structure.toml in this change";
const FIX_FUNCTION: &str = "split the function, or raise thresholds.function_lines/function_complexity in structure.toml in this change";
const FIX_HOTSPOT: &str = "split complexity out of this hotspot, or add 'Code-health-allow: hotspot <path> — <reason>'";
const FIX_GENERATED: &str = "restore the producer, or remove the [[generated]] claim in structure.toml";
pub(super) const UNMAPPED: &str = "new hand source file in no module — add it to a [[module]] paths glob in structure.toml, or move it under one";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Violation {
    pub(super) rule: String,
    pub(super) path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) line: Option<usize>,
    pub(super) text: String,
}

impl Violation {
    pub(super) fn new(rule: &str, path: &str, text: String) -> Self {
        Self { rule: rule.to_owned(), path: path.to_owned(), line: None, text }
    }

    pub(super) fn shown(&self) -> String {
        let line = self.line.filter(|line| *line > 0).map_or_else(String::new, |line| format!(":{line}"));
        format!("{} {}{line} {}", self.rule, self.path, self.text)
    }
}

pub(super) fn layer_text(to: &str, from_module: &str, to_module: &str) -> String {
    format!("new import -> {to}: module {from_module} may not use {to_module} — {FIX_LAYER}")
}

pub(super) fn cycle_text(to: &str) -> String {
    format!("new import -> {to} closes an import cycle — {FIX_CYCLE}")
}

pub(super) fn file_size(path: &str, head: usize, base: Option<usize>, limit: f64) -> Option<Violation> {
    if head as f64 <= limit || base.is_some_and(|base| base as f64 > limit) {
        return None;
    }
    let was = base.map_or_else(|| "new".to_owned(), |base| format!("was {base}"));
    Some(Violation::new("file-size", path, format!("{head} lines > {limit} ({was}) — {FIX_FILE}")))
}

pub(super) fn function_debt(path: &str, head: &[Function], base: &[Function], t: &Thresholds) -> Vec<Violation> {
    let before: HashMap<&str, &Function> = base.iter().map(|f| (f.name.as_str(), f)).collect();
    let ok = |f: &Function| f.lines as f64 <= t.function_lines && f.complexity as f64 <= t.function_complexity;
    let mut out = Vec::new();
    for f in head.iter().filter(|f| !ok(f)) {
        let previous = before.get(f.name.as_str());
        if previous.is_some_and(|b| !ok(b)) {
            continue;
        }
        let was = previous.map_or_else(|| "new".to_owned(), |b| format!("was {} lines, complexity {}", b.lines, b.complexity));
        let text = format!("{} {} lines (limit {}), complexity {} (limit {}) ({was}) — {FIX_FUNCTION}", f.name, f.lines, t.function_lines, f.complexity, t.function_complexity);
        out.push(Violation { line: Some(f.line), ..Violation::new("function", path, text) });
    }
    out
}

struct Change {
    path: String,
    base: Option<String>,
}

fn changes(repo: &Path, base: &str, head: &str) -> Result<Vec<Change>, String> {
    let listing = git(repo, &["diff", "-M", "--name-status", "-z", base, head])?;
    let parts: Vec<&str> = listing.split('\0').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        let status = parts[i].chars().next().unwrap_or(' ');
        i += 1;
        if matches!(status, 'R' | 'C') {
            let (from, to) = (parts[i], parts.get(i + 1).copied().unwrap_or_default());
            i += 2;
            out.push(Change { path: to.to_owned(), base: (status == 'R').then(|| from.to_owned()) });
        } else {
            let path = parts[i];
            i += 1;
            if status != 'D' {
                out.push(Change { path: path.to_owned(), base: (status != 'A').then(|| path.to_owned()) });
            }
        }
    }
    Ok(out)
}

pub(super) struct GateResult {
    pub(super) violations: Vec<Violation>,
    pub(super) warning: Option<String>,
}

fn complexity(functions: Option<&Vec<Function>>) -> Option<usize> {
    functions.map(|functions| functions.iter().map(|f| f.complexity).sum())
}

pub(super) fn gate(state: &State, repo: &Path, base: &str, head: &str, override_path: Option<&Path>) -> Result<GateResult, String> {
    let (mut hs, mut bs) = (Snapshot::new(repo, head)?, Snapshot::new(repo, base)?);
    let config = at_rev(&hs, override_path)?;
    if !config.present {
        return Ok(GateResult { violations: Vec::new(), warning: Some("code-health: no structure.toml at HEAD; gate skipped".to_owned()) });
    }
    let base_config = at_rev(&bs, override_path)?;
    let head_kinds: HashMap<String, Kind> = inventory(&hs, &config).into_iter().collect();
    let changed: Vec<Change> = changes(repo, base, head)?.into_iter().filter(|c| head_kinds.get(&c.path) == Some(&Kind::Hand)).collect();
    let mut v = generated_debt(&config, &base_config, &hs, &bs);
    if changed.is_empty() {
        return Ok(GateResult { violations: v, warning: None });
    }
    let head_files: Vec<SourceText> = super::report::texts(&mut hs, &changed.iter().map(|c| c.path.clone()).collect::<Vec<_>>())?;
    let based: Vec<&Change> = changed.iter().filter(|c| c.base.as_ref().is_some_and(|b| bs.has(b))).collect();
    let base_paths: Vec<String> = based.iter().filter_map(|c| c.base.clone()).collect();
    let base_files: Vec<SourceText> = super::report::texts(&mut bs, &base_paths)?;
    let base_as_head: Vec<SourceText> = based.iter().zip(&base_files).map(|(c, file)| SourceText { path: c.path.clone(), text: file.text.clone() }).collect();
    for (c, file) in changed.iter().zip(&head_files) {
        let before = c.base.as_ref().and_then(|b| bs.text(b)).map(|text| line_count(&text));
        v.extend(file_size(&c.path, line_count(&file.text), before, config.thresholds_of(&c.path).file_lines));
    }
    let measured = measure(&[("h", &head_files), ("b", &base_as_head)])?;
    let head_of = |c: &Change| measured.get(&format!("h/{}", c.path));
    let base_of = |c: &Change| measured.get(&format!("b/{}", c.path));
    for c in &changed {
        v.extend(function_debt(&c.path, head_of(c).map_or(&[][..], Vec::as_slice), base_of(c).map_or(&[][..], Vec::as_slice), &config.thresholds_of(&c.path)));
    }
    let renamed: HashMap<&str, &str> = changed.iter().filter_map(|c| c.base.as_deref().filter(|b| *b != c.path).map(|b| (b, c.path.as_str()))).collect();
    let head_resolver = Resolver::of_snapshot(&hs);
    let base_resolver = Resolver::of_snapshot(&bs);
    let base_imports: HashMap<&str, Vec<String>> = base_files.iter().map(|f| (f.path.as_str(), base_resolver.imports(&f.path, &f.text))).collect();
    let mut added: Vec<(String, String)> = Vec::new();
    for (c, file) in changed.iter().zip(&head_files) {
        let before: HashSet<String> = c.base.as_deref().and_then(|b| base_imports.get(b)).map(|found| found.iter().map(|x| renamed.get(x.as_str()).map_or_else(|| x.clone(), |to| (*to).to_owned())).collect()).unwrap_or_default();
        for to in head_resolver.imports(&c.path, &file.text) {
            if before.contains(&to) || head_kinds.get(&to) != Some(&Kind::Hand) {
                continue;
            }
            if let Some(lv) = layer_violation(&config, &c.path, &to) {
                v.push(Violation::new("layer", &c.path, layer_text(&to, &lv.from_module, &lv.to_module)));
            }
            added.push((c.path.clone(), to));
        }
        if c.base.is_none() && !config.modules.is_empty() && config.module_of(&c.path).is_none() {
            v.push(Violation::new("layer", &c.path, UNMAPPED.to_owned()));
        }
    }
    if !added.is_empty() {
        let hand = hand_of(&inventory(&hs, &config));
        let cycles = sccs(&import_graph(&mut hs, &hand, &head_resolver)?);
        let component = component_of(&cycles);
        for (path, to) in &added {
            if component.get(path.as_str()).is_some_and(|k| component.get(to.as_str()) == Some(k)) {
                v.push(Violation::new("cycle", path, cycle_text(to)));
            }
        }
    }
    let rising: Vec<&Change> = based.iter().copied().filter(|c| complexity(head_of(c)).unwrap_or(0) > complexity(base_of(c)).unwrap_or(0)).collect();
    if !rising.is_empty() {
        let baseline = state.baseline(repo);
        let files = baseline.as_ref().and_then(|b| b.get("files")).filter(|files| files.is_object());
        let measured_base: HashMap<&str, usize> = changed.iter().filter_map(|c| c.base.as_deref().map(|b| (b, complexity(base_of(c)).unwrap_or(0)))).collect();
        let base_hand: HashMap<String, Kind> = inventory(&bs, &base_config).into_iter().collect();
        let of = |p: &str| {
            if base_hand.get(p) != Some(&Kind::Hand) {
                return None;
            }
            match files {
                Some(files) => measured_base.get(p).copied().or_else(|| files.get(p).and_then(|row| row.get("complexity")).and_then(serde_json::Value::as_u64).map(|n| n as usize)),
                None => Some(1),
            }
        };
        let ranked = hotspots_of(&churn_at(state, repo, base)?, of);
        for c in rising {
            let limit = config.thresholds_of(&c.path).hotspot_top;
            if !ranked.iter().take(limit as usize).any(|h| Some(&h.path) == c.base.as_ref()) {
                continue;
            }
            let (now, was) = (complexity(head_of(c)).unwrap_or(0), complexity(base_of(c)).unwrap_or(0));
            v.push(Violation::new("hotspot", &c.path, format!("complexity {now} > {was} in a top-{limit} hotspot — {FIX_HOTSPOT}")));
        }
    }
    let allowed = allows(repo, &[&format!("{base}..{head}")])?;
    v.retain(|x| !(["file-size", "function", "hotspot"].contains(&x.rule.as_str()) && allowed.iter().any(|a| a.rule == x.rule && a.path == x.path)));
    Ok(GateResult { violations: v, warning: None })
}

fn generated_debt(config: &Config, base_config: &Config, hs: &Snapshot, bs: &Snapshot) -> Vec<Violation> {
    let mut v = Vec::new();
    for g in config.generated.iter().filter(|g| !hs.has(&g.producer)) {
        let claimed_before = base_config.generated.iter().any(|b| b.producer == g.producer && bs.has(&g.producer));
        let is_new = !base_config.generated.iter().any(|b| b.producer == g.producer);
        if claimed_before || is_new {
            v.push(Violation::new("generated", &g.producer, format!("producer missing at HEAD; {} lose the generated exemption — {FIX_GENERATED}", g.paths.join(", "))));
        }
    }
    v
}
