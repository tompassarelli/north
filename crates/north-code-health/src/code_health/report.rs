use super::config::{Config, Kind, Thresholds};
use super::functions::{Function, SourceText, measure};
use super::graph::{Graph, LayerViolation, fan, layer_violation, module_graph, sccs};
use super::history::{Allow, Pair, WINDOW_COMMITS, WINDOW_DAYS, allows, co_change, history};
use super::resolver::Resolver;
use super::snapshot::{Snapshot, git, line_count};
use super::state::State;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub(super) fn inventory(snapshot: &Snapshot, config: &Config) -> Vec<(String, Kind)> {
    snapshot.paths().iter().map(|path| (path.clone(), config.classify(path, |other| snapshot.has(other)))).filter(|(_, kind)| *kind != Kind::Other).collect()
}

pub(super) fn hand_of(kinds: &[(String, Kind)]) -> Vec<String> {
    kinds.iter().filter(|(_, kind)| *kind == Kind::Hand).map(|(path, _)| path.clone()).collect()
}

pub(super) fn texts(snapshot: &mut Snapshot, paths: &[String]) -> Result<Vec<SourceText>, String> {
    snapshot.load(paths)?;
    Ok(paths.iter().map(|path| SourceText { path: path.clone(), text: snapshot.text(path).unwrap_or_default() }).collect())
}

pub(super) fn import_graph(snapshot: &mut Snapshot, hand: &[String], resolver: &Resolver) -> Result<Graph, String> {
    let files = texts(snapshot, hand)?;
    let hand_set: HashSet<&str> = hand.iter().map(String::as_str).collect();
    let mut graph = Graph::default();
    for (file, found) in files.iter().zip(resolver.imports_all(&files)) {
        graph.insert(&file.path);
        for target in found.iter().filter(|target| hand_set.contains(target.as_str())) {
            graph.link(&file.path, target);
        }
    }
    Ok(graph)
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Distribution {
    p50: usize,
    p90: usize,
    p99: usize,
    max: usize,
    n: usize,
}

fn distribution(values: impl Iterator<Item = usize>) -> Distribution {
    let mut sorted: Vec<usize> = values.collect();
    sorted.sort_unstable();
    let at = |q: f64| if sorted.is_empty() { 0 } else { sorted[((q * sorted.len() as f64).ceil() as usize).saturating_sub(1).min(sorted.len() - 1)] };
    Distribution { p50: at(0.5), p90: at(0.9), p99: at(0.99), max: sorted.last().copied().unwrap_or(0), n: sorted.len() }
}

#[derive(Debug, Serialize)]
pub(super) struct FileRow {
    pub(super) lines: usize,
    pub(super) complexity: usize,
    pub(super) functions: Vec<Function>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) module: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Hotspot {
    pub(super) path: String,
    pub(super) commits: usize,
    pub(super) complexity: usize,
    pub(super) score: usize,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Count {
    files: usize,
    lines: usize,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Split {
    hand: Count,
    generated: Count,
    ignored: Count,
}

#[derive(Debug, Serialize)]
pub(super) struct Distributions {
    file_lines: Distribution,
    function_lines: Distribution,
    function_complexity: Distribution,
}

#[derive(Debug, Serialize)]
pub(super) struct Over {
    files: usize,
    function_lines: usize,
    function_complexity: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct HistoryWindow {
    commits: usize,
    days: i64,
    max_commits: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct Report {
    pub(super) repo: String,
    pub(super) sha: String,
    pub(super) at: String,
    thresholds: Thresholds,
    split: Split,
    distributions: Distributions,
    over: Over,
    files: BTreeMap<String, FileRow>,
    unmapped: Vec<String>,
    cycles: Vec<Vec<String>>,
    layer_violations: Vec<LayerViolation>,
    module_graph: BTreeMap<String, Vec<String>>,
    fan_in: Vec<(String, usize)>,
    fan_out: Vec<(String, usize)>,
    module_fan_in: Vec<(String, usize)>,
    module_fan_out: Vec<(String, usize)>,
    co_change: Vec<Pair>,
    pub(super) hotspots: Vec<Hotspot>,
    history: HistoryWindow,
    allowed_debt: Vec<Allow>,
    #[serde(skip)]
    order: Vec<String>,
}

fn collation(c: char) -> (u8, u32) {
    const PUNCTUATION: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if c.is_whitespace() {
        (0, c as u32)
    } else if let Some(at) = PUNCTUATION.find(c) {
        (1, at as u32)
    } else if c.is_ascii_digit() {
        (2, c as u32)
    } else if c.is_alphabetic() {
        (3, c.to_lowercase().next().unwrap_or(c) as u32)
    } else {
        (1, 64 + c as u32)
    }
}

pub(super) fn locale_cmp(a: &str, b: &str) -> Ordering {
    let primary = a.chars().map(collation).cmp(b.chars().map(collation));
    primary.then_with(|| a.chars().map(|c| c.is_uppercase()).cmp(b.chars().map(|c| c.is_uppercase())))
}

fn ranked(counts: &[(String, usize)]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = counts.iter().filter(|(_, n)| *n > 0).cloned().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| locale_cmp(&a.0, &b.0)));
    out
}

pub(super) fn hotspots_of(churn: &[(String, usize)], complexity: impl Fn(&str) -> Option<usize>) -> Vec<Hotspot> {
    let mut out: Vec<Hotspot> = churn.iter().filter_map(|(path, commits)| complexity(path).map(|c| Hotspot { path: path.clone(), commits: *commits, complexity: c, score: commits * c })).collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| locale_cmp(&a.path, &b.path)));
    out
}

pub(super) fn now() -> String {
    north_base::foundation::clock::Timestamp::now().iso_millis()
}

pub(super) fn build(repo: &Path, rev: &str, config: &Config) -> Result<Report, String> {
    let mut snapshot = Snapshot::new(repo, rev)?;
    let sha = git(repo, &["rev-parse", rev])?.trim().to_owned();
    let kinds = inventory(&snapshot, config);
    let all: Vec<String> = kinds.iter().map(|(path, _)| path.clone()).collect();
    snapshot.load(&all)?;
    let mut split = Split::default();
    let mut lines: HashMap<String, usize> = HashMap::new();
    for (path, kind) in &kinds {
        let n = line_count(&snapshot.text(path).unwrap_or_default());
        lines.insert(path.clone(), n);
        let count = match kind {
            Kind::Hand => &mut split.hand,
            Kind::Generated => &mut split.generated,
            _ => &mut split.ignored,
        };
        count.files += 1;
        count.lines += n;
    }
    let hand = hand_of(&kinds);
    let sources = texts(&mut snapshot, &hand)?;
    let mut measured = measure(&[("h", &sources)])?;
    let mut files: BTreeMap<String, FileRow> = BTreeMap::new();
    for path in &hand {
        let functions = measured.remove(&format!("h/{path}")).unwrap_or_default();
        let complexity = functions.iter().map(|f| f.complexity).sum();
        files.insert(path.clone(), FileRow { lines: lines[path], complexity, functions, module: config.module_of(path).map(|m| m.name.clone()) });
    }
    let resolver = Resolver::of_snapshot(&snapshot);
    let graph = import_graph(&mut snapshot, &hand, &resolver)?;
    let layer: Vec<LayerViolation> = graph.nodes().flat_map(|(from, targets)| targets.iter().filter_map(move |to| layer_violation(config, from, to))).collect();
    let modules = module_graph(config, &graph);
    let file_fan = fan(&graph);
    let module_fan = fan(&modules);
    let past = history(repo, rev)?;
    let hand_set: HashSet<&str> = hand.iter().map(String::as_str).collect();
    let functions: Vec<&Function> = hand.iter().flat_map(|path| files[path].functions.iter()).collect();
    let over = |path: &String, test: &dyn Fn(&Function, &Thresholds) -> bool| files[path].functions.iter().filter(|f| test(f, &config.thresholds_of(path))).count();
    let hotspot_top = config.modules.iter().map(|m| m.thresholds.hotspot_top).fold(config.thresholds.hotspot_top.max(10.0), f64::max) as usize;
    let grouped = !config.modules.is_empty();
    Ok(Report {
        repo: repo.to_string_lossy().into_owned(),
        sha,
        at: now(),
        thresholds: config.thresholds,
        split,
        distributions: Distributions {
            file_lines: distribution(hand.iter().map(|path| files[path].lines)),
            function_lines: distribution(functions.iter().map(|f| f.lines)),
            function_complexity: distribution(functions.iter().map(|f| f.complexity)),
        },
        over: Over {
            files: hand.iter().filter(|path| files[*path].lines as f64 > config.thresholds_of(path).file_lines).count(),
            function_lines: hand.iter().map(|path| over(path, &|f, t| f.lines as f64 > t.function_lines)).sum(),
            function_complexity: hand.iter().map(|path| over(path, &|f, t| f.complexity as f64 > t.function_complexity)).sum(),
        },
        unmapped: if grouped { hand.iter().filter(|path| files[*path].module.is_none()).cloned().collect() } else { Vec::new() },
        cycles: sccs(&graph),
        layer_violations: layer,
        module_graph: modules.nodes().map(|(name, targets)| {
            let mut targets = targets.to_vec();
            targets.sort();
            (name.to_owned(), targets)
        }).collect(),
        fan_in: ranked(&file_fan.fan_in).into_iter().take(10).collect(),
        fan_out: ranked(&file_fan.fan_out).into_iter().take(10).collect(),
        module_fan_in: ranked(&module_fan.fan_in),
        module_fan_out: ranked(&module_fan.fan_out),
        co_change: co_change(&past, |path| hand_set.contains(path), |path| if grouped { config.module_of(path).map(|m| m.name.clone()) } else { None }).into_iter().take(10).collect(),
        hotspots: hotspots_of(&past.churn, |path| files.get(path).map(|row| row.complexity)).into_iter().take(hotspot_top).collect(),
        history: HistoryWindow { commits: past.commits, days: WINDOW_DAYS, max_commits: WINDOW_COMMITS },
        allowed_debt: allows(repo, &["-n", &WINDOW_COMMITS.to_string(), rev])?,
        files,
        order: hand,
    })
}

#[derive(Serialize)]
pub(super) struct TrendRow<'a> {
    at: &'a str,
    sha: &'a str,
    hand_files: usize,
    hand_lines: usize,
    generated_lines: usize,
    over: &'a Over,
    cycles: usize,
    layer_violations: usize,
    top_hotspot: usize,
    allowed_debt: usize,
}

pub(super) fn trend_row(report: &Report) -> TrendRow<'_> {
    TrendRow {
        at: &report.at,
        sha: &report.sha,
        hand_files: report.split.hand.files,
        hand_lines: report.split.hand.lines,
        generated_lines: report.split.generated.lines,
        over: &report.over,
        cycles: report.cycles.len(),
        layer_violations: report.layer_violations.len(),
        top_hotspot: report.hotspots.first().map_or(0, |h| h.score),
        allowed_debt: report.allowed_debt.len(),
    }
}

fn shown(d: &Distribution) -> String {
    format!("p50 {}  p90 {}  p99 {}  max {}  (n={})", d.p50, d.p90, d.p99, d.max, d.n)
}

pub(super) fn format(report: &Report, state: &State) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("code-health report {} @ {}", state.repo_name(Path::new(&report.repo)), &report.sha[..report.sha.len().min(12)]));
    out.extend(["".to_owned(), "Split (files / lines)".to_owned()]);
    for (name, count) in [("hand", &report.split.hand), ("generated", &report.split.generated), ("ignored", &report.split.ignored)] {
        out.push(format!("  {name:<10} {:>6} {:>9}", count.files, count.lines));
    }
    out.extend(["".to_owned(), "Distributions".to_owned()]);
    out.push(format!("  file lines        {}", shown(&report.distributions.file_lines)));
    out.push(format!("  function lines    {}", shown(&report.distributions.function_lines)));
    out.push(format!("  complexity        {}", shown(&report.distributions.function_complexity)));
    out.push(format!("  over effective limits  files: {}  function lines: {}  complexity: {}", report.over.files, report.over.function_lines, report.over.function_complexity));
    out.extend(["".to_owned(), "Top 10 files by size".to_owned()]);
    let mut by_size: Vec<&String> = report.order.iter().collect();
    by_size.sort_by(|a, b| report.files[*b].lines.cmp(&report.files[*a].lines));
    for path in by_size.into_iter().take(10) {
        out.push(format!("  {:>6}  {path}", report.files[path].lines));
    }
    out.extend(["".to_owned(), "Top 10 functions by complexity (complexity, lines)".to_owned()]);
    let mut functions: Vec<(&String, &Function)> = report.order.iter().flat_map(|path| report.files[path].functions.iter().map(move |f| (path, f))).collect();
    functions.sort_by(|a, b| b.1.complexity.cmp(&a.1.complexity).then(b.1.lines.cmp(&a.1.lines)));
    for (path, f) in functions.into_iter().take(10) {
        out.push(format!("  {:>4} {:>5}  {path}:{} {}", f.complexity, f.lines, f.line, f.name));
    }
    out.extend(["".to_owned(), format!("Cycles: {}", report.cycles.len())]);
    for cycle in report.cycles.iter().take(10) {
        let shown: Vec<&str> = cycle.iter().take(6).map(String::as_str).collect();
        out.push(format!("  [{}] {}{}", cycle.len(), shown.join(" "), if cycle.len() > 6 { " ..." } else { "" }));
    }
    let unmapped = if report.unmapped.is_empty() { String::new() } else { format!("   unmapped hand files: {}", report.unmapped.len()) };
    out.extend(["".to_owned(), format!("Layer violations: {}{unmapped}", report.layer_violations.len())]);
    for v in report.layer_violations.iter().take(10) {
        out.push(format!("  {} -> {}  {} -> {}", v.from_module, v.to_module, v.from, v.to));
    }
    for path in report.unmapped.iter().take(5) {
        out.push(format!("  unmapped {path}"));
    }
    for (title, rows) in [("Top fan-in (files)", &report.fan_in), ("Top fan-out (files)", &report.fan_out)] {
        out.extend(["".to_owned(), title.to_owned()]);
        for (path, n) in rows {
            out.push(format!("  {n:>4}  {path}"));
        }
    }
    if !report.module_fan_in.is_empty() || !report.module_fan_out.is_empty() {
        let pairs = |rows: &[(String, usize)]| rows.iter().map(|(m, n)| format!("{m}={n}")).collect::<Vec<_>>().join(" ");
        out.extend(["".to_owned(), "Module fan-in / fan-out".to_owned()]);
        out.push(format!("  in:  {}", pairs(&report.module_fan_in)));
        out.push(format!("  out: {}", pairs(&report.module_fan_out)));
    }
    out.extend(["".to_owned(), format!("Top co-change pairs ({} commits, support / confidence)", report.history.commits)]);
    for c in &report.co_change {
        out.push(format!("  {:>4} {:.2}  {} <-> {}", c.support, c.confidence, c.a, c.b));
    }
    out.extend(["".to_owned(), "Top 10 hotspots (commits x complexity)".to_owned()]);
    for h in report.hotspots.iter().take(10) {
        out.push(format!("  {:>7}  {:>4} x {:>5}  {}", h.score, h.commits, h.complexity, h.path));
    }
    if !report.allowed_debt.is_empty() {
        out.extend(["".to_owned(), "Allowed debt (Code-health-allow)".to_owned()]);
        for a in &report.allowed_debt {
            out.push(format!("  {} {} {} {}", a.rule, a.path, &a.commit[..a.commit.len().min(10)], a.reason));
        }
    }
    out.join("\n")
}
