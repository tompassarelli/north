use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use super::workers::repos;
use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;
use crate::router::pricing::PriceTable;

pub const BAR: f64 = 0.70;
/// The harness, its private policy and the machines it runs on: every other repository is a product.
const HARNESS: [&str; 3] = ["north", "south", "firn"];
const SOURCE: &str = "worker-ledger runs ended in the window (cost API-equivalent from router/prices.json) and those that landed in it, grouped by the repository their item names (threads runs); north, south and firn are the harness, every other repository a product; a printed bar, nothing refuses or reorders work on it (north#871)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counted {
    Dollars,
    Runs,
}

/// What one window's worker runs weigh, in all and in each repository their items name; a run naming several repositories weighs fully in each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Product {
    pub product: f64,
    pub total: f64,
    pub by_repo: BTreeMap<String, f64>,
    pub runs: usize,
}

impl Product {
    fn add(&mut self, repos: &BTreeSet<String>, weight: f64) {
        if repos.is_empty() {
            return;
        }
        self.runs += 1;
        self.total += weight;
        if repos.iter().any(|repo| !is_harness(repo)) {
            self.product += weight;
        }
        for repo in repos {
            *self.by_repo.entry(repo.clone()).or_default() += weight;
        }
    }

    pub fn share(&self) -> Option<f64> {
        (self.total > 0.0).then_some(self.product / self.total)
    }

    pub fn text(&self, counted: Counted) -> String {
        let amount = |value: f64| match counted {
            Counted::Dollars => format!("${value:.2}"),
            Counted::Runs => format!("{value:.0}"),
        };
        let Some(share) = self.share() else { return "no record".into() };
        let repos = self.by_repo.iter().map(|(repo, weight)| format!("{repo} {}", amount(*weight))).collect::<Vec<_>>().join(", ");
        format!("{:.0}% ({} of {}; {repos})", share * 100.0, amount(self.product), amount(self.total))
    }
}

fn is_harness(repo: &str) -> bool {
    HARNESS.contains(&repo.rsplit('/').next().unwrap_or(repo))
}

fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse_python_datetime(text).map(Timestamp::unix_seconds)
}

pub fn product_share(db: &Connection, weeks: [Window; 2]) -> Result<[Measure; 2], String> {
    let prices = PriceTable::load().map_err(|why| format!("prices: {why}"))?;
    let mut statement = db
        .prepare(&format!(
            "SELECT coalesce(item, ''), tier, model, coalesce(input_tokens, 0), coalesce(cache_read_tokens, 0), coalesce(tokens, 0), ended, coalesce(landed_sha, ''), coalesce(nullif(landed_at, ''), ended) \
             FROM runs WHERE {} AND outcome != 'running' AND ended != ''",
            worker_where()
        ))
        .map_err(|why| why.to_string())?;
    let mut spent = [Product::default(), Product::default()];
    let mut landed = [Product::default(), Product::default()];
    let rows = statement
        .query_map([], |row| {
            let model: Option<String> = row.get(2)?;
            let usd = prices.run_usd(model.as_deref(), &row.get::<_, String>(1)?, row.get(3)?, row.get(4)?, row.get(5)?);
            Ok((row.get::<_, String>(0)?, usd, row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?))
        })
        .map_err(|why| why.to_string())?;
    for (item, usd, ended, sha, landed_at) in rows.filter_map(Result::ok) {
        let repos = repos(&item);
        for (index, window) in weeks.iter().enumerate() {
            if seconds(&ended).is_some_and(|at| window.holds(at)) {
                spent[index].add(&repos, usd);
            }
            if !sha.is_empty() && seconds(&landed_at).is_some_and(|at| window.holds(at)) {
                landed[index].add(&repos, 1.0);
            }
        }
    }
    let measure = |name: &str, counted: Counted, weeks: [Product; 2]| Measure {
        name: format!("{name} (bar {:.0}%)", BAR * 100.0),
        better: Better::Higher,
        source: SOURCE,
        weeks: weeks.map(|product| Figure::Product { product, counted }),
    };
    Ok([measure("product share of worker spend", Counted::Dollars, spent), measure("product share of landed worker runs", Counted::Runs, landed)])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Counted, Product, is_harness};

    #[test]
    fn a_repository_outside_north_south_and_firn_is_a_product_with_no_edit() {
        let names = |repos: &[&str]| repos.iter().map(|repo| (*repo).to_owned()).collect::<BTreeSet<_>>();
        let mut week = Product::default();
        week.add(&names(&["north"]), 3.0);
        week.add(&names(&["wisp"]), 1.0);
        week.add(&names(&["brand-new-game"]), 1.0);
        week.add(&names(&[]), 9.0);
        assert!(is_harness("tompassarelli/firn") && !is_harness("makgora"));
        assert_eq!(week.share(), Some(0.4));
        assert_eq!(week.text(Counted::Runs), "40% (2 of 5; brand-new-game 1, north 3, wisp 1)");
    }
}
