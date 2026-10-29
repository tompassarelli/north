//! `north advisor-report`: compares policy-replay arms (model, effort, advisor) against an incumbent: pass rate, paired
//! task-cluster difference, list and plan $ per success, wall time, advisor use and one verdict per challenger.
mod stats;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use toml_edit::DocumentMut;

use super::report::wilson;
use stats::{PyRandom, Row, float_sum, paired_diff, upper_quantile};

const USAGE: &str = "usage: north advisor-report FILE... --incumbent ARM [--arm-key FIELD] [--codex-plan-usd ARM=VALUE]...";
const RESAMPLES: usize = 4000;
const DEFAULTS: &str = include_str!("../../../harness/defaults.toml");

pub(super) struct Run {
    scenario: String,
    passed: Option<bool>,
    usd: Option<f64>,
    wall_s: Option<f64>,
    codex: bool,
    advisor_calls: f64,
    advisor_usd: f64,
}

struct Arm {
    n: usize,
    rate: f64,
    wilson: (f64, f64),
    list_run: Option<f64>,
    list_success: Option<f64>,
    plan_run: Option<f64>,
    plan_success: Option<f64>,
    plan_success_hi: Option<f64>,
    wall_median: Option<f64>,
    advisor_mean: f64,
    advisor_share_runs: f64,
    advisor_usd_share: Option<f64>,
}

struct Options {
    files: Vec<PathBuf>,
    arm_key: Option<String>,
    incumbent: String,
    codex_plan: BTreeMap<String, f64>,
}

fn default(name: &str) -> f64 {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    let value = &defaults[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { files: Vec::new(), arm_key: None, incumbent: String::new(), codex_plan: BTreeMap::new() };
    let mut incumbent = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--arm-key" => options.arm_key = Some(value("--arm-key")?),
            "--incumbent" => incumbent = Some(value("--incumbent")?),
            "--codex-plan-usd" => {
                let item = value("--codex-plan-usd")?;
                let (arm, amount) = item.split_once('=').ok_or_else(|| format!("--codex-plan-usd {item:?} is not ARM=VALUE"))?;
                options.codex_plan.insert(arm.to_owned(), amount.parse().map_err(|_| format!("--codex-plan-usd {item:?} is not ARM=VALUE"))?);
            }
            flag if flag.starts_with("--") => return Err(format!("unrecognized arguments: {flag}")),
            file => options.files.push(PathBuf::from(file)),
        }
    }
    if options.files.is_empty() {
        return Err("the following arguments are required: files".into());
    }
    options.incumbent = incumbent.ok_or("the following arguments are required: --incumbent")?;
    Ok(options)
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".into(),
        Value::Bool(flag) => if *flag { "True" } else { "False" }.into(),
        other => other.to_string(),
    }
}

fn run_of(row: &Value) -> Result<Run, String> {
    let number = |name: &str| row.get(name).and_then(Value::as_f64);
    Ok(Run {
        scenario: text(row.get("scenario").ok_or("a row has no scenario")?),
        passed: match row.get("pass") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(flag)) => Some(*flag),
            Some(_) => return Err("a row's pass is not true, false or null".into()),
        },
        usd: number("usd"),
        wall_s: number("wall_s"),
        codex: row.get("provider").and_then(Value::as_str) == Some("codex"),
        advisor_calls: number("advisor_calls").unwrap_or(0.0),
        advisor_usd: number("advisor_usd").unwrap_or(0.0),
    })
}

fn load(paths: &[PathBuf], arm_key: Option<&str>) -> Result<Vec<(String, Vec<Run>)>, String> {
    let mut arms: Vec<(String, Vec<Run>)> = Vec::new();
    for path in paths {
        let content = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            let row: Value = serde_json::from_str(line).map_err(|error| format!("{}: {error}", path.display()))?;
            let name = match arm_key {
                Some(key) => text(row.get(key).ok_or_else(|| format!("a row has no {key}"))?),
                None => path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
            };
            let run = run_of(&row)?;
            match arms.iter_mut().find(|(arm, _)| *arm == name) {
                Some((_, runs)) => runs.push(run),
                None => arms.push((name, vec![run])),
            }
        }
    }
    Ok(arms)
}

fn plan_costs(runs: &[Run], codex_plan_usd: Option<f64>) -> Vec<Option<f64>> {
    let codex = runs.iter().filter(|run| run.codex).count();
    let ratio = default("plan_per_list_claude");
    runs.iter().map(|run| if run.codex { codex_plan_usd.map(|total| total / codex as f64) } else { Some(run.usd.unwrap_or(0.0) * ratio) }).collect()
}

fn per(total: Option<f64>, count: usize) -> Option<f64> {
    total.filter(|_| count > 0).map(|total| total / count as f64)
}

fn plan_upper(runs: &[Run], plans: &[Option<f64>]) -> Option<f64> {
    let mut clusters: BTreeMap<&str, (f64, u32)> = BTreeMap::new();
    for (run, plan) in runs.iter().zip(plans) {
        let cell = clusters.entry(run.scenario.as_str()).or_default();
        cell.0 += (*plan)?;
        cell.1 += u32::from(run.passed == Some(true));
    }
    let cells: Vec<(f64, u32)> = clusters.into_values().collect();
    if cells.is_empty() {
        return None;
    }
    let mut rng = PyRandom::seeded_zero();
    let mut boot: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let pick: Vec<(f64, u32)> = cells.iter().map(|_| cells[rng.below(cells.len())]).collect();
            let wins: u32 = pick.iter().map(|cell| cell.1).sum();
            if wins == 0 { f64::INFINITY } else { float_sum(pick.iter().map(|cell| cell.0)) / f64::from(wins) }
        })
        .collect();
    boot.sort_by(f64::total_cmp);
    Some(upper_quantile(&boot))
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    match values.len() {
        0 => None,
        n if n % 2 == 1 => Some(values[mid]),
        _ => Some((values[mid - 1] + values[mid]) / 2.0),
    }
}

fn summarize(runs: &[Run], codex_plan_usd: Option<f64>) -> Arm {
    let scored: Vec<bool> = runs.iter().filter_map(|run| run.passed).collect();
    let wins = scored.iter().filter(|passed| **passed).count();
    let rate = if scored.is_empty() { 0.0 } else { wins as f64 / scored.len() as f64 };
    let priced: Vec<f64> = runs.iter().filter_map(|run| run.usd).collect();
    let list_total = (!priced.is_empty()).then(|| float_sum(priced.iter().copied()));
    let plans = plan_costs(runs, codex_plan_usd);
    let plan_total = plans.iter().copied().collect::<Option<Vec<f64>>>().map(float_sum);
    let claude_usd = float_sum(runs.iter().filter(|run| !run.codex).map(|run| run.usd.unwrap_or(0.0)));
    let calls: Vec<f64> = runs.iter().map(|run| run.advisor_calls).collect();
    let share = |count: usize| if calls.is_empty() { 0.0 } else { count as f64 / calls.len() as f64 };
    Arm {
        n: scored.len(),
        rate,
        wilson: wilson(rate, scored.len()),
        list_run: per(list_total, priced.len()),
        list_success: per(list_total, wins),
        plan_run: per(plan_total, runs.len()),
        plan_success: per(plan_total, wins),
        plan_success_hi: plan_upper(runs, &plans),
        wall_median: median(runs.iter().filter_map(|run| run.wall_s).collect()),
        advisor_mean: if calls.is_empty() { 0.0 } else { float_sum(calls.iter().copied()) / calls.len() as f64 },
        advisor_share_runs: share(calls.iter().filter(|count| **count > 0.0).count()),
        advisor_usd_share: (claude_usd != 0.0).then(|| float_sum(runs.iter().map(|run| run.advisor_usd)) / claude_usd),
    }
}

fn verdict(challenger: &Arm, incumbent: &Arm, diff: (f64, f64, f64), margin: f64) -> &'static str {
    let (_, low, high) = diff;
    if challenger.wilson.0 > incumbent.wilson.1 {
        return "BEATS";
    }
    let cheaper = matches!((challenger.plan_success_hi, incumbent.plan_success), (Some(hi), Some(plan)) if hi < plan);
    if low >= -margin && cheaper {
        "MATCHES-FOR-LESS"
    } else if high < margin {
        "NO-GAIN"
    } else {
        "UNDECIDED"
    }
}

fn money(value: Option<f64>) -> String {
    match value {
        None => "-".into(),
        Some(amount) if amount.is_infinite() => "inf".into(),
        Some(amount) => format!("{amount:.4}"),
    }
}

fn pct(value: Option<f64>) -> String {
    value.map_or_else(|| "-".into(), |share| format!("{:.1}%", 100.0 * share))
}

fn rows_of(runs: &[Run]) -> Vec<Row> {
    runs.iter().map(|run| Row { scenario: run.scenario.clone(), passed: run.passed }).collect()
}

fn render(arms: &[(String, Vec<Run>)], incumbent: &str, codex_plan: &BTreeMap<String, f64>) -> String {
    let stats: Vec<Arm> = arms.iter().map(|(name, runs)| summarize(runs, codex_plan.get(name).copied())).collect();
    let position = arms.iter().position(|(name, _)| name == incumbent).expect("incumbent was checked");
    let incumbent_rows = rows_of(&arms[position].1);
    let margin = default("noninferiority_margin") / 100.0;
    let head = ["arm".to_owned(), "n".into(), "pass".into(), "wilson95".into(), format!("diff vs {incumbent}"), "list$/run".into(), "list$/win".into(), "plan$/run".into(), "plan$/win".into(), "plan$/win hi".into(), "wall_med".into(), "adv mean".into(), "adv runs".into(), "adv $".into()];
    let mut table = vec![head.to_vec()];
    let mut verdicts = Vec::new();
    for (index, ((name, runs), s)) in arms.iter().zip(&stats).enumerate() {
        let diff = (index != position).then(|| paired_diff(&rows_of(runs), &incumbent_rows, RESAMPLES));
        if let Some(diff) = diff {
            verdicts.push(format!("{name}: {} vs {incumbent}", verdict(s, &stats[position], diff, margin)));
        }
        table.push(vec![
            name.clone(),
            s.n.to_string(),
            pct(Some(s.rate)),
            format!("[{}, {}]", pct(Some(s.wilson.0)), pct(Some(s.wilson.1))),
            diff.map_or_else(|| "incumbent".into(), |d| format!("{:+.1} [{:+.1}, {:+.1}]", 100.0 * d.0, 100.0 * d.1, 100.0 * d.2)),
            money(s.list_run),
            money(s.list_success),
            money(s.plan_run),
            money(s.plan_success),
            money(s.plan_success_hi),
            s.wall_median.map_or_else(|| "-".into(), |wall| format!("{wall:.1}")),
            format!("{:.2}", s.advisor_mean),
            pct(Some(s.advisor_share_runs)),
            pct(s.advisor_usd_share),
        ]);
    }
    let widths: Vec<usize> = (0..head.len()).map(|column| table.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    let mut lines: Vec<String> = table
        .iter()
        .map(|row| row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_owned())
        .collect();
    lines.extend(verdicts);
    lines.join("\n")
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{USAGE}\nnorth advisor-report: error: {message}");
            return 2;
        }
    };
    let arms = match load(&options.files, options.arm_key.as_deref()) {
        Ok(arms) => arms,
        Err(message) => {
            eprintln!("advisor-report: {message}");
            return 1;
        }
    };
    if !arms.iter().any(|(name, _)| *name == options.incumbent) {
        let names: Vec<&str> = { let mut names: Vec<&str> = arms.iter().map(|(name, _)| name.as_str()).collect(); names.sort_unstable(); names };
        eprintln!("{USAGE}\nnorth advisor-report: error: incumbent {:?} is not one of {names:?}", options.incumbent);
        return 2;
    }
    println!("{}", render(&arms, &options.incumbent, &options.codex_plan));
    0
}
