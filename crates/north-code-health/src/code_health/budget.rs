use super::change_cost::ChangeCost;
use super::config::Config;
use super::coupling::{Coupling, VOLATILE_MIN_COMMITS, VOLATILITY_RATIO, coupling};
use super::crate_lines;
use super::rust_lines;
use super::signal::{cost_line, cost_since_review};
use super::state::State;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use std::sync::LazyLock;
use toml_edit::DocumentMut;

const DEFAULTS: &str = "harness/defaults.toml";
const RUST_LINE_BUDGET_KEY: &str = "rust_line_budget";

static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|/)(tests?\.rs|[a-z_]*_tests?\.rs|tests/)").expect("test path pattern"));
static EVALS_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|/)evals/").expect("evals path pattern"));

#[derive(Serialize)]
pub(super) struct RustLines {
    lines: usize,
    budget: usize,
}

#[derive(Serialize)]
pub(super) struct Budget {
    rust_lines: Option<RustLines>,
    crates: Vec<String>,
    coupling: Coupling,
    since: Option<String>,
    change_cost: ChangeCost,
}

pub(super) fn budget(state: &State, repo: &Path, rev: &str, config: &Config) -> Result<Budget, String> {
    let (since, change_cost) = cost_since_review(state, repo, rev, config)?;
    let rust_lines = measure_rust_lines(state, repo, rev)?;
    Ok(Budget { rust_lines, crates: crate_lines::lines_at(repo, rev)?, coupling: coupling(repo, rev, config)?, since, change_cost })
}

fn measure_rust_lines(state: &State, repo: &Path, rev: &str) -> Result<Option<RustLines>, String> {
    let Some(budget) = read_tunable(repo)? else { return Ok(None) };
    let lines = rust_lines::count(&state.rust_line_cache(), repo, rev, |path| !EVALS_PATH.is_match(path) && !TEST_PATH.is_match(path))?;
    Ok(Some(RustLines { lines, budget }))
}

fn read_tunable(repo: &Path) -> Result<Option<usize>, String> {
    match std::fs::read_to_string(repo.join(DEFAULTS)) {
        Ok(defaults) => parse_tunable(&defaults),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {DEFAULTS}: {error}")),
    }
}

fn parse_tunable(defaults: &str) -> Result<Option<usize>, String> {
    let document: DocumentMut = defaults.parse().map_err(|error| format!("{DEFAULTS} does not parse: {error}"))?;
    let Some(value) = document.get(RUST_LINE_BUDGET_KEY).and_then(|table| table.get("value")) else { return Ok(None) };
    value.as_integer().and_then(|value| usize::try_from(value).ok()).map(Some).ok_or_else(|| format!("{DEFAULTS} has no whole number for {RUST_LINE_BUDGET_KEY}"))
}

fn rust_line_budget_line(rust: &RustLines) -> String {
    let status = if rust.lines > rust.budget { format!("over by {}", rust.lines - rust.budget) } else { "within".to_owned() };
    format!("Rust line budget: {} non-test lines against {} ({status}; north#1276, counted by the command in harness/defaults.toml [{RUST_LINE_BUDGET_KEY}])", rust.lines, rust.budget)
}

pub(super) fn format(budget: &Budget) -> String {
    let mut out: Vec<String> = match &budget.rust_lines {
        Some(rust) => vec![rust_line_budget_line(rust), "".to_owned()],
        None => Vec::new(),
    };
    out.push("Crate budgets (non-test lines against structure.toml [crate_lines])".to_owned());
    out.extend(budget.crates.iter().map(|line| format!("  {}", line.trim_start_matches("crate-lines: "))));
    let c = &budget.coupling;
    out.extend(["".to_owned(), format!("Hidden links: {} (module pairs whose files change together with no may_use edge either way)", c.hidden_links.len())]);
    out.extend(c.hidden_links.iter().map(|link| format!("  {:>4}  {} <-> {}  e.g. {}", link.support, link.a, link.b, link.example)));
    out.extend(["".to_owned(), format!("Volatile imports: {} (a module importing one that changed at least {VOLATILITY_RATIO} times as often and in at least {VOLATILE_MIN_COMMITS} commits of the history window)", c.volatile_imports.len())]);
    out.extend(c.volatile_imports.iter().map(|edge| format!("  {} ({} commits) -> {} ({} commits)", edge.from, edge.from_commits, edge.to, edge.to_commits)));
    out.extend(["".to_owned(), format!("Special cases: {} (swallowed errors / sleeps and retries / disabled tests per module)", c.special_cases_total())]);
    let width = c.special_cases.keys().map(String::len).max().unwrap_or(0);
    out.extend(c.special_cases.iter().map(|(module, n)| format!("  {module:width$}  {:>4} {:>4} {:>4}", n.swallowed_errors, n.sleeps_and_retries, n.disabled_tests)));
    out.extend(["".to_owned(), format!("Change cost: {}", cost_line(budget.since.as_deref(), &budget.change_cost))]);
    out.join("\n")
}
