//! JSON verbs for the harness scripts that replay the ledger offline: `annotate-runs`, `recommend-runs` and `calibration-runs`.
use std::collections::BTreeMap;
use std::io::Read;

use serde_json::{Value, json};

use super::read::calibration_runs;
use super::recommend::recommend;
use super::{GROUPS, Knobs, RUN_TIERS, Run, cell_value, tier_key, tier_rank};
use crate::threads::runs::brief::{CATEGORIES, FACETS, Fields};
use crate::threads::verb::{Context, Failure, Outcome, open};

fn request(verb: &str, stdin: &mut dyn Read) -> Result<Value, Failure> {
    let mut input = String::new();
    stdin.read_to_string(&mut input).map_err(|error| Failure::Refused(format!("{verb} cannot read stdin: {error}")))?;
    serde_json::from_str(&input).map_err(|error| Failure::Usage(format!("{verb} reads JSON on stdin: {error}")))
}

fn text<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

/// A run row's cells, tier key and rank, and whether its brief names the category and facets its columns hold.
fn annotation(row: &Value) -> Value {
    let cells: Vec<String> = GROUPS.iter().map(|group| cell_value(group, text(row, group))).collect();
    let tier = tier_key(text(row, "tier").unwrap_or_default());
    let fields = Fields::read(text(row, "brief_text").unwrap_or_default());
    let named = [fields.category.as_deref(), fields.facets[0].as_deref(), fields.facets[1].as_deref(), fields.facets[2].as_deref(), fields.facets[3].as_deref()];
    let vocabulary = text(row, "category").is_some_and(|category| CATEGORIES.contains(&category))
        && FACETS.iter().all(|(name, values)| text(row, &name.to_lowercase()).is_some_and(|value| values.contains(&value)));
    let covered = vocabulary && GROUPS.iter().zip(named).all(|(group, named)| named.is_some() && named == text(row, group));
    json!({"known_tier": RUN_TIERS.iter().any(|(key, _)| *key == tier), "rank": tier_rank(&tier), "tier": tier, "cells": cells, "brief_covered": covered})
}

pub(super) fn annotate(stdin: &mut dyn Read) -> Outcome {
    let rows = request("annotate-runs", stdin)?;
    let rows = rows.as_array().ok_or_else(|| Failure::Usage("annotate-runs reads a list of run rows".into()))?;
    Ok(format!("{}\n", json!({"rows": rows.iter().map(annotation).collect::<Vec<_>>()})))
}

fn run_of(row: &Value) -> Run {
    let cells: Vec<String> = row.get("cells").and_then(Value::as_array).map(|cells| cells.iter().map(|cell| cell.as_str().unwrap_or("-").to_owned()).collect()).unwrap_or_default();
    Run {
        cells: std::array::from_fn(|index| cells.get(index).cloned().unwrap_or_else(|| "-".to_owned())),
        tier: tier_key(text(row, "tier").unwrap_or_default()),
        minutes: row.get("minutes").and_then(Value::as_f64),
        cost_usd: row.get("cost_usd").and_then(Value::as_f64),
        outcome: text(row, "outcome").unwrap_or_default().to_owned(),
        ok: row.get("ok").and_then(Value::as_bool).unwrap_or(false),
        ..Run::default()
    }
}

/// Each case's recommendation over its runs; with `carry`, a case's pick is held as the next cases' incumbent for its cell.
pub(super) fn recommend_cases(stdin: &mut dyn Read) -> Outcome {
    let request = request("recommend-runs", stdin)?;
    let cases = request.get("cases").and_then(Value::as_array).ok_or_else(|| Failure::Usage("recommend-runs needs a cases list".into()))?;
    let carry = request.get("carry").and_then(Value::as_bool).unwrap_or(false);
    let knobs = Knobs::registry();
    let mut held: BTreeMap<String, String> = BTreeMap::new();
    let mut picks = Vec::new();
    for case in cases {
        let runs: Vec<Run> = case.get("runs").and_then(Value::as_array).map(|runs| runs.iter().map(run_of).collect()).unwrap_or_default();
        let asked: [Option<String>; 5] = std::array::from_fn(|index| case.get("asked").and_then(|asked| asked.get(index)).and_then(Value::as_str).map(str::to_owned));
        let incumbent: BTreeMap<String, String> = if carry {
            held.clone()
        } else {
            case.get("incumbent").and_then(Value::as_object).map(|held| held.iter().filter_map(|(cell, tier)| Some((cell.clone(), tier.as_str()?.to_owned()))).collect()).unwrap_or_default()
        };
        let pick = recommend(&knobs, &runs, &asked, &incumbent, true);
        if carry {
            match &pick.tier {
                Some(tier) => held.insert(pick.cell.clone(), tier.clone()),
                None => held.remove(&pick.cell),
            };
        }
        let stats: Vec<Value> = pick.stats.iter().map(|(tier, stat)| json!({"tier": tier, "rank": tier_rank(tier), "n": stat.n, "rate": stat.rate, "ok": stat.ok, "usd_n": stat.usd_n, "usd": stat.usd})).collect();
        picks.push(json!({"cell": pick.cell, "n": pick.n, "tier": pick.tier, "rule": pick.rule, "stats": stats}));
    }
    Ok(format!("{}\n", json!({"picks": picks})))
}

/// Every settled worker run's tier, cost and minutes, as `threads calibrate` reads them.
pub(super) fn runs(context: &Context) -> Outcome {
    let db = open(context.database, false)?;
    let rows: Vec<Value> = calibration_runs(&db)?.into_iter().map(|run| json!({"tier": run.tier, "cost_usd": run.cost_usd, "minutes": run.minutes})).collect();
    Ok(format!("{}\n", Value::Array(rows)))
}
