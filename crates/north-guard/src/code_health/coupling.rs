use super::config::Config;
use super::graph::module_graph;
use super::history::{History, co_change, history};
use super::report::{hand_of, import_graph, inventory, texts};
use super::resolver::Resolver;
use super::snapshot::Snapshot;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

pub(super) const VOLATILITY_RATIO: usize = 3;
pub(super) const VOLATILE_MIN_COMMITS: usize = 30;

static SWALLOWED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\blet _ =|\.ok\(\);|\bErr\(_\)\s*=>|\bcatch\s*(\(\s*\w*\s*\))?\s*\{\s*\}|\.catch\(\s*\(\)\s*=>\s*(\{\s*\}|undefined|null)\s*\)").unwrap());
static WAITED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bsleep\w*\s*\(|\bsetTimeout\s*\(|(?i)retr(y|ies|ying)").unwrap());
static DISABLED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\[ignore\b|\b(it|test|describe)\.(skip|todo)\(|\bx(it|describe)\(").unwrap());

#[derive(Debug, Serialize)]
pub(super) struct HiddenLink {
    pub(super) a: String,
    pub(super) b: String,
    pub(super) support: usize,
    pub(super) example: String,
}

#[derive(Debug, Serialize)]
pub(super) struct VolatileImport {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) from_commits: usize,
    pub(super) to_commits: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct SpecialCases {
    pub(super) swallowed_errors: usize,
    pub(super) sleeps_and_retries: usize,
    pub(super) disabled_tests: usize,
}

impl SpecialCases {
    pub(super) const fn total(self) -> usize {
        self.swallowed_errors + self.sleeps_and_retries + self.disabled_tests
    }
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Coupling {
    pub(super) hidden_links: Vec<HiddenLink>,
    pub(super) volatile_imports: Vec<VolatileImport>,
    pub(super) special_cases: BTreeMap<String, SpecialCases>,
}

impl Coupling {
    pub(super) fn special_cases_total(&self) -> usize {
        self.special_cases.values().map(|cases| cases.total()).sum()
    }
}

pub(super) fn coupling(repo: &Path, rev: &str, config: &Config) -> Result<Coupling, String> {
    if config.modules.is_empty() {
        return Ok(Coupling::default());
    }
    let mut snapshot = Snapshot::new(repo, rev)?;
    let hand = hand_of(&inventory(&snapshot, config));
    let resolver = Resolver::of_snapshot(&snapshot);
    let graph = import_graph(&mut snapshot, &hand, &resolver)?;
    let sources = texts(&mut snapshot, &hand)?;
    let past = history(repo, rev)?;
    let hand_set: HashSet<&str> = hand.iter().map(String::as_str).collect();
    let commits = module_commits(config, &past, &hand_set);
    let modules = module_graph(config, &graph);
    let mut volatile_imports: Vec<VolatileImport> = modules
        .nodes()
        .flat_map(|(from, targets)| targets.iter().map(move |to| (from, to.as_str())))
        .map(|(from, to)| VolatileImport { from: from.to_owned(), to: to.to_owned(), from_commits: commits.get(from).copied().unwrap_or(0), to_commits: commits.get(to).copied().unwrap_or(0) })
        .filter(|edge| edge.to_commits >= VOLATILE_MIN_COMMITS && edge.to_commits >= VOLATILITY_RATIO * edge.from_commits.max(1))
        .collect();
    volatile_imports.sort_by(|x, y| (y.to_commits - y.from_commits).cmp(&(x.to_commits - x.from_commits)).then_with(|| x.from.cmp(&y.from)));
    let mut special_cases: BTreeMap<String, SpecialCases> = BTreeMap::new();
    for file in &sources {
        if let Some(module) = config.module_of(&file.path) {
            let cases = special_cases.entry(module.name.clone()).or_default();
            *cases = add(*cases, count(&file.text));
        }
    }
    special_cases.retain(|_, cases| cases.total() > 0);
    Ok(Coupling { hidden_links: hidden_links(config, &past, &hand_set), volatile_imports, special_cases })
}

fn hidden_links(config: &Config, past: &History, hand: &HashSet<&str>) -> Vec<HiddenLink> {
    let mut links: BTreeMap<(String, String), HiddenLink> = BTreeMap::new();
    for pair in co_change(past, |path| hand.contains(path), |path| config.module_of(path).map(|module| module.name.clone())) {
        let (Some(a), Some(b)) = (config.module_of(&pair.a), config.module_of(&pair.b)) else { continue };
        if a.may_use.contains(&b.name) || b.may_use.contains(&a.name) {
            continue;
        }
        let key = if a.name < b.name { (a.name.clone(), b.name.clone()) } else { (b.name.clone(), a.name.clone()) };
        let link = links.entry(key.clone()).or_insert_with(|| HiddenLink { a: key.0, b: key.1, support: 0, example: format!("{} <-> {}", pair.a, pair.b) });
        link.support += pair.support;
    }
    let mut out: Vec<HiddenLink> = links.into_values().collect();
    out.sort_by(|x, y| y.support.cmp(&x.support).then_with(|| x.a.cmp(&y.a)));
    out
}

fn module_commits(config: &Config, past: &History, hand: &HashSet<&str>) -> HashMap<String, usize> {
    let mut commits: HashMap<String, usize> = HashMap::new();
    for files in past.changes() {
        let touched: HashSet<&str> = files.iter().filter(|path| hand.contains(path.as_str())).filter_map(|path| config.module_of(path)).map(|module| module.name.as_str()).collect();
        for module in touched {
            *commits.entry(module.to_owned()).or_default() += 1;
        }
    }
    commits
}

pub(super) fn count(text: &str) -> SpecialCases {
    let lines = |pattern: &Regex| text.lines().filter(|line| pattern.is_match(line)).count();
    SpecialCases { swallowed_errors: lines(&SWALLOWED), sleeps_and_retries: lines(&WAITED), disabled_tests: lines(&DISABLED) }
}

const fn add(a: SpecialCases, b: SpecialCases) -> SpecialCases {
    SpecialCases { swallowed_errors: a.swallowed_errors + b.swallowed_errors, sleeps_and_retries: a.sleeps_and_retries + b.sleeps_and_retries, disabled_tests: a.disabled_tests + b.disabled_tests }
}
