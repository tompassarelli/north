use super::change_cost::ChangeCost;
use super::config::Config;
use super::coupling::{Coupling, VOLATILE_MIN_COMMITS, VOLATILITY_RATIO, coupling};
use super::crate_lines;
use super::signal::{cost_line, cost_since_review};
use super::state::State;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub(super) struct Budget {
    crates: Vec<String>,
    coupling: Coupling,
    since: Option<String>,
    change_cost: ChangeCost,
}

pub(super) fn budget(state: &State, repo: &Path, rev: &str, config: &Config) -> Result<Budget, String> {
    let (since, change_cost) = cost_since_review(state, repo, rev, config)?;
    Ok(Budget { crates: crate_lines::lines_at(repo, rev)?, coupling: coupling(repo, rev, config)?, since, change_cost })
}

pub(super) fn format(budget: &Budget) -> String {
    let mut out: Vec<String> = vec!["Crate budgets (non-test lines against structure.toml [crate_lines])".to_owned()];
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
