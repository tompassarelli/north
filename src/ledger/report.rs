use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::NorthResult;

use super::{Ledger, Run, Task};

const PRICES: &str = include_str!("prices.json");
const UNMEASURED: &str = "unmeasured";

#[derive(Deserialize)]
struct PriceTable {
    models: HashMap<String, Price>,
}

#[derive(Clone, Copy, Deserialize)]
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

impl Price {
    fn of(self, run: &Run) -> f64 {
        let cost = |tokens: i64, rate: f64| tokens as f64 * rate;
        (cost(run.tokens_in, self.input)
            + cost(run.tokens_out, self.output)
            + cost(run.tokens_cache_read, self.cache_read)
            + cost(run.tokens_cache_write, self.cache_write))
            / 1_000_000.0
    }
}

pub fn print_runs(ledger: &Ledger, json: bool) -> NorthResult<()> {
    let runs = ledger.runs()?;
    if json {
        println!("{}", serde_json::to_string(&runs)?);
        return Ok(());
    }
    for run in &runs {
        println!(
            "{}\t{}\t{}/{}\t{}\t{}\twall {}\tturns {}\ttokens {}/{}/{}/{}\ttask {}",
            run.run_id,
            run.session,
            run.provider,
            run.model.as_deref().unwrap_or("-"),
            run.account.as_deref().unwrap_or("-"),
            run.exit,
            seconds(run.wall_ms),
            run.turns,
            run.tokens_in,
            run.tokens_out,
            run.tokens_cache_read,
            run.tokens_cache_write,
            run.task_ref.as_deref().unwrap_or("-"),
        );
    }
    println!("{} runs", runs.len());
    Ok(())
}

pub fn print_summary(ledger: &Ledger, json: bool) -> NorthResult<()> {
    let prices: PriceTable = serde_json::from_str(PRICES)?;
    let mut groups: BTreeMap<(String, String), Vec<Run>> = BTreeMap::new();
    for run in ledger.runs()? {
        let key = (run.provider.clone(), run.model.clone().unwrap_or_else(|| "-".into()));
        groups.entry(key).or_default().push(run);
    }
    let models: Vec<Value> = groups.iter().map(|((provider, model), runs)| group(provider, model, runs, prices.models.get(model).copied())).collect();
    let tasks = ledger.tasks()?;
    if json {
        println!("{}", json!({"models": models, "tasks": tasks}));
        return Ok(());
    }
    models.iter().for_each(print_group);
    tasks.iter().for_each(print_task);
    Ok(())
}

fn group(provider: &str, model: &str, runs: &[Run], price: Option<Price>) -> Value {
    let count = runs.len();
    let mut walls: Vec<i64> = runs.iter().map(|run| run.wall_ms).collect();
    walls.sort_unstable();
    let sum = |field: fn(&Run) -> i64| runs.iter().map(field).sum::<i64>();
    json!({
        "provider": provider,
        "model": model,
        "runs": count,
        "wall_ms": {"p25": percentile(&walls, 25), "p50": percentile(&walls, 50), "p75": percentile(&walls, 75), "runs": count},
        "tokens": {
            "input": sum(|run| run.tokens_in),
            "output": sum(|run| run.tokens_out),
            "cache_read": sum(|run| run.tokens_cache_read),
            "cache_write": sum(|run| run.tokens_cache_write),
            "runs": count,
        },
        "api_usd": {"usd": price.map(|price| runs.iter().map(|run| price.of(run)).sum::<f64>()), "runs": if price.is_some() { count } else { 0 }},
        "plan_usd": {"usd": UNMEASURED, "runs": count},
    })
}

fn percentile(sorted: &[i64], percent: usize) -> Option<i64> {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

fn print_group(group: &Value) {
    let wall = &group["wall_ms"];
    let tokens = &group["tokens"];
    let api = match group["api_usd"]["usd"].as_f64() {
        Some(usd) => format!("${usd:.4}"),
        None => "unpriced".into(),
    };
    println!(
        "{}/{}\truns {}\twall p50 {} [p25 {} – p75 {}] (n={})\ttokens in/out/cache_read/cache_write {}/{}/{}/{} (n={})\tapi {} (n={})\tplan $ {} (n={})",
        text(&group["provider"]),
        text(&group["model"]),
        group["runs"],
        seconds_of(&wall["p50"]),
        seconds_of(&wall["p25"]),
        seconds_of(&wall["p75"]),
        wall["runs"],
        tokens["input"],
        tokens["output"],
        tokens["cache_read"],
        tokens["cache_write"],
        tokens["runs"],
        api,
        group["api_usd"]["runs"],
        text(&group["plan_usd"]["usd"]),
        group["plan_usd"]["runs"],
    );
}

fn print_task(task: &Task) {
    let check = match (task.last_check_exit, &task.last_check_sha) {
        (Some(exit), sha) => format!("last check exit {exit} at {} (n={})", sha.as_deref().unwrap_or("no HEAD"), task.checks),
        (None, _) => "no checks (n=0)".into(),
    };
    println!(
        "task {}\t{} {}\twall {}\tsessions {}\t{check}",
        task.task_ref,
        task.outcome.as_deref().unwrap_or("open"),
        task.landed_sha.as_deref().unwrap_or(""),
        task.wall_ms.map_or_else(|| "-".into(), seconds),
        task.sessions,
    );
}

fn seconds_of(value: &Value) -> String {
    value.as_i64().map_or_else(|| "-".into(), seconds)
}

fn seconds(millis: i64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let walls = [10, 20, 30, 40];
        assert_eq!([25, 50, 75].map(|percent| percentile(&walls, percent)), [Some(10), Some(20), Some(30)]);
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn every_price_parses() {
        let table: PriceTable = serde_json::from_str(PRICES).unwrap();
        assert!(table.models.contains_key("claude-haiku-5-5") && table.models.contains_key("gpt-6-luna"));
    }
}
