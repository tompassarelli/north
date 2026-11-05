use std::collections::BTreeMap;

use rusqlite::Connection;

use super::model::{ModelRow, Quantiles, level_of, rows};

pub const DRIFT_ERROR: f64 = 0.5;
pub const COVERAGE_TARGET: f64 = 0.8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationRun {
    pub key: String,
    pub host: String,
    pub basis: Option<String>,
    pub predicted_wall: Option<f64>,
    pub predicted_wall_p90: Option<f64>,
    pub actual_wall: Option<f64>,
    pub predicted_memory: Option<f64>,
    pub actual_memory: Option<f64>,
    pub censored: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub key: String,
    pub host: String,
    pub runs: usize,
    pub censored: usize,
    pub basis: String,
    pub predicted_wall: Quantiles,
    pub actual_wall: Quantiles,
    pub wall_error: Quantiles,
    pub wall_coverage: Option<f64>,
    pub predicted_memory: Quantiles,
    pub actual_memory: Quantiles,
    pub memory_error: Quantiles,
    pub memory_coverage: Option<f64>,
    pub drift: bool,
}

fn measured(pair: (Option<f64>, Option<f64>)) -> Option<(f64, f64)> {
    match pair {
        (Some(predicted), Some(actual)) if predicted.is_finite() && actual.is_finite() && actual > 0.0 => Some((predicted, actual)),
        _ => None,
    }
}

fn errors(runs: &[&CalibrationRun], pick: fn(&CalibrationRun) -> (Option<f64>, Option<f64>)) -> Vec<f64> {
    runs.iter().filter_map(|run| measured(pick(run))).map(|(predicted, actual)| (predicted - actual).abs() / actual).collect()
}

fn coverage(runs: &[&CalibrationRun], pick: fn(&CalibrationRun) -> (Option<f64>, Option<f64>)) -> Option<f64> {
    let pairs: Vec<(f64, f64)> = runs.iter().filter_map(|run| measured(pick(run))).collect();
    (!pairs.is_empty()).then(|| pairs.iter().filter(|(bound, actual)| actual <= bound).count() as f64 / pairs.len() as f64)
}

fn most_common(runs: &[&CalibrationRun]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for run in runs {
        *counts.entry(run.basis.as_deref().unwrap_or("-")).or_default() += 1;
    }
    counts.into_iter().max_by(|left, right| left.1.cmp(&right.1).then(right.0.cmp(left.0))).map_or_else(|| "-".to_owned(), |(basis, _)| basis.to_owned())
}

pub fn calibrate(runs: &[CalibrationRun]) -> Vec<Calibration> {
    let mut grouped: BTreeMap<(&str, &str), Vec<&CalibrationRun>> = BTreeMap::new();
    for run in runs {
        grouped.entry((run.key.as_str(), run.host.as_str())).or_default().push(run);
    }
    grouped
        .into_iter()
        .map(|((key, host), runs)| {
            let values = |pick: fn(&CalibrationRun) -> Option<f64>| runs.iter().filter_map(|run| pick(run)).filter(|value| value.is_finite()).collect::<Vec<f64>>();
            let wall_error = Quantiles::of(&errors(&runs, |run| (run.predicted_wall, run.actual_wall)));
            let memory_error = Quantiles::of(&errors(&runs, |run| if run.censored { (None, None) } else { (run.predicted_memory, run.actual_memory) }));
            let wall_coverage = coverage(&runs, |run| (run.predicted_wall_p90, run.actual_wall));
            let memory_coverage = coverage(&runs, |run| match (run.censored, run.predicted_memory, run.actual_memory) {
                (true, Some(predicted), Some(actual)) if actual <= predicted => (None, None),
                (_, predicted, actual) => (predicted, actual),
            });
            let short = |covered: Option<f64>| covered.is_some_and(|covered| covered < COVERAGE_TARGET);
            Calibration {
                key: key.to_owned(),
                host: host.to_owned(),
                runs: runs.len(),
                censored: runs.iter().filter(|run| run.censored).count(),
                basis: most_common(&runs),
                predicted_wall: Quantiles::of(&values(|run| run.predicted_wall)),
                actual_wall: Quantiles::of(&values(|run| run.actual_wall)),
                wall_error,
                wall_coverage,
                predicted_memory: Quantiles::of(&values(|run| run.predicted_memory)),
                actual_memory: Quantiles::of(&values(|run| run.actual_memory)),
                memory_error,
                memory_coverage,
                drift: wall_error.p90 > DRIFT_ERROR || memory_error.p90 > DRIFT_ERROR || short(wall_coverage) || short(memory_coverage),
            }
        })
        .collect()
}

pub fn calibration_runs(db: &Connection, since: &str) -> rusqlite::Result<Vec<CalibrationRun>> {
    let mut statement = db.prepare(
        "SELECT key, host, predictionBasis, predictedWallSeconds, predictedWallP90Seconds, wallSeconds, predictedMemoryMiB, anonPeakMiB, censored
         FROM capacity_runs WHERE key IS NOT NULL AND at >= ? ORDER BY at, source_id",
    )?;
    statement
        .query_map([since], |row| {
            Ok(CalibrationRun {
                key: row.get(0)?,
                host: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                basis: row.get(2)?,
                predicted_wall: row.get(3)?,
                predicted_wall_p90: row.get(4)?,
                actual_wall: row.get(5)?,
                predicted_memory: row.get(6)?,
                actual_memory: row.get(7)?,
                censored: row.get::<_, Option<i64>>(8)?.is_some_and(|flag| flag != 0),
            })
        })?
        .collect()
}

pub fn parse_window_seconds(text: &str) -> Option<f64> {
    let split = text.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let (amount, unit) = text.split_at(split);
    let amount: f64 = amount.parse().ok()?;
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86_400.0,
        _ => return None,
    };
    (amount > 0.0).then_some(amount * scale)
}

fn cell(value: f64) -> String {
    if !value.is_finite() {
        "-".to_owned()
    } else if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn pair(quantiles: Quantiles) -> String {
    format!("{}/{}", cell(quantiles.p50), cell(quantiles.p90))
}

fn share(value: f64) -> String {
    if value.is_finite() { format!("{:.0}%", value * 100.0) } else { "-".to_owned() }
}

fn percent(quantiles: Quantiles) -> String {
    format!("{}/{}", share(quantiles.p50), share(quantiles.p90))
}

pub fn model_table(rows: &[ModelRow]) -> String {
    let mut text = format!("{:<18} {:<48} {:<10} {:<6} {:>5} {:>13} {:>13} {:>15} {:>8}\n", "LEVEL", "KEY", "HOST", "CACHE", "RUNS", "WALL s", "CPU s", "ANON MiB", "FLOOR");
    for row in rows {
        let key: String = row.key.split_once(':').map_or(row.key.as_str(), |(_, rest)| rest).chars().take(48).collect();
        text.push_str(&format!(
            "{:<18} {:<48} {:<10} {:<6} {:>5} {:>13} {:>13} {:>15} {:>8}\n",
            level_of(&row.key, &row.cache),
            key,
            row.host,
            row.cache,
            row.runs,
            pair(row.wall),
            pair(row.cpu),
            pair(row.memory),
            row.floor_mib.map_or_else(|| "-".to_owned(), cell)
        ));
    }
    text.push_str("p50/p90 over each row's newest runs\n");
    text
}

pub fn calibration_table(calibrations: &[Calibration]) -> String {
    let header = ["KEY", "HOST", "RUNS", "CENS", "BASIS", "PRED WALL", "ACT WALL", "WALL ERR", "WALL COV", "PRED MEM", "ACT MEM", "MEM ERR", "MEM COV", "DRIFT"];
    let line = |cells: [String; 14]| {
        format!(
            "{:<40} {:<8} {:>4} {:>4} {:<18} {:>11} {:>11} {:>11} {:>8} {:>11} {:>11} {:>11} {:>7} {}\n",
            cells[0], cells[1], cells[2], cells[3], cells[4], cells[5], cells[6], cells[7], cells[8], cells[9], cells[10], cells[11], cells[12], cells[13]
        )
    };
    let mut text = line(header.map(str::to_owned));
    for row in calibrations {
        let covered = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), share);
        text.push_str(&line([
            row.key.chars().take(40).collect(),
            row.host.clone(),
            row.runs.to_string(),
            row.censored.to_string(),
            row.basis.clone(),
            pair(row.predicted_wall),
            pair(row.actual_wall),
            percent(row.wall_error),
            covered(row.wall_coverage),
            pair(row.predicted_memory),
            pair(row.actual_memory),
            percent(row.memory_error),
            covered(row.memory_coverage),
            (if row.drift { "DRIFT" } else { "ok" }).to_owned(),
        ]));
    }
    text.push_str("wall s and anon MiB as p50/p90; error |predicted-actual|/actual p50/p90 (wall p50, memory p90); coverage: share of runs ending at or under the predicted p90; a censored run (CENS) is a memory lower bound: no memory error, and a coverage miss only when it already passed the prediction; BASIS unknown: no level of the key had 3 runs on this host\n");
    text
}

const USAGE: &str = "usage: north capacity model [KEY] | north capacity calibration [--since 24h]";

pub fn model_command(arguments: &[String]) -> u8 {
    if arguments.len() > 1 {
        eprintln!("{USAGE}");
        return 2;
    }
    match super::database::capacity_database().and_then(|db| rows(&db, arguments.first().map(String::as_str))) {
        Ok(rows) => {
            print!("{}", model_table(&rows));
            0
        }
        Err(error) => {
            eprintln!("north capacity model: {error}");
            super::database::failure_status(&error) as u8
        }
    }
}

pub fn calibration_command(arguments: &[String]) -> u8 {
    let window = match arguments {
        [] => Some(86_400.0),
        [flag, since] if flag == "--since" => parse_window_seconds(since),
        _ => None,
    };
    let Some(window) = window else {
        eprintln!("{USAGE}");
        return 2;
    };
    let now = crate::foundation::clock::Timestamp::now().unix_millis();
    let since = crate::foundation::clock::Timestamp::from_unix_millis(now - (window * 1000.0) as i64).iso_millis();
    match super::database::capacity_database().and_then(|db| calibration_runs(&db, &since)) {
        Ok(runs) => {
            print!("{}", calibration_table(&calibrate(&runs)));
            0
        }
        Err(error) => {
            eprintln!("north capacity calibration: {error}");
            super::database::failure_status(&error) as u8
        }
    }
}

#[cfg(test)]
#[path = "calibration_tests.rs"]
mod tests;
