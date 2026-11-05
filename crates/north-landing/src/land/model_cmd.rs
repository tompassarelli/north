use std::fmt::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::candidate::git;
use super::lander::state_home;
use super::{outcomes, queue};
use crate::land_model::buckets::{Target, Weights};
use crate::land_model::features;
use crate::land_model::replay::{replay, Report, Scoring};
use crate::land_model::{Era, Lane};

const USAGE: &str = "usage: north land model [--repo DIR] [--era batch|precheck] [--base REF] [--json]";
const ERAS: [Era; 2] = [Era::Batch, Era::Precheck];
const SCORINGS: [Scoring; 2] = [Scoring::LeaveOneBranchOut, Scoring::Prequential];

struct Options {
    repo: PathBuf,
    era: Option<Era>,
    base: String,
    json: bool,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { repo: PathBuf::from("."), era: None, base: "origin/main".into(), json: false };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            "--base" => options.base.clone_from(rest.next()?),
            "--json" => options.json = true,
            "--era" => {
                options.era = Some(match rest.next()?.as_str() {
                    "batch" => Era::Batch,
                    "precheck" => Era::Precheck,
                    _ => return None,
                })
            }
            _ => return None,
        }
    }
    Some(options)
}

pub struct Loaded {
    pub lanes: Vec<Lane>,
    pub tickets: usize,
    pub skipped: usize,
    pub ledger_lines: usize,
}

pub fn load(queue: &Path, repository: &Path, base: &str) -> Result<Loaded, String> {
    let records = features::load_done(&queue.join("done")).map_err(|error| format!("{}: {error}", queue.join("done").display()))?;
    let ledger = outcomes::read(queue).map_err(|error| format!("{}: {error}", outcomes::path(queue).display()))?;
    let tickets = records.len();
    let collected = features::collect(repository, base, outcomes::join(records, &ledger));
    Ok(Loaded { lanes: features::lanes(&collected.rows), tickets, skipped: collected.skipped, ledger_lines: ledger.lines.len() })
}

pub fn reports(lanes: &[Lane], era: Option<Era>) -> Vec<Report> {
    let mut reports = Vec::new();
    for target in Target::ALL {
        for era in ERAS.into_iter().filter(|candidate| era.is_none_or(|wanted| wanted == *candidate)) {
            for scoring in SCORINGS {
                reports.push(replay(lanes, target, era, scoring, Weights::default()));
            }
        }
    }
    reports
}

pub fn render(loaded: &Loaded, reports: &[Report]) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "north land model: {} done tickets, {} skipped without a resolvable head, {} ledger lines", loaded.tickets, loaded.skipped, loaded.ledger_lines);
    for report in reports {
        let _ = writeln!(
            text,
            "\n{} {} {}: n={} base rate={:.3}\n  brier model={:.3} base={:.3}  log loss model={:.3} base={:.3}",
            report.target.name(),
            report.era.name(),
            report.scoring.name(),
            report.rows,
            report.base_rate,
            report.brier_model,
            report.brier_base,
            report.log_loss_model,
            report.log_loss_base
        );
        for decile in &report.deciles {
            let _ = writeln!(text, "  decile rows={} predicted={:.3} observed={:.3} se={:.3}", decile.rows, decile.mean_predicted, decile.observed_rate, decile.standard_error);
        }
        for cell in &report.cells {
            let _ = writeln!(text, "  cell {}: weight={:.2} events={:.2} posterior={:.3}", cell.label, cell.weight, cell.events, cell.posterior);
        }
    }
    text
}

pub fn render_json(loaded: &Loaded, reports: &[Report]) -> Value {
    let reports: Vec<Value> = reports
        .iter()
        .map(|report| {
            json!({
                "target": report.target.name(),
                "era": report.era.name(),
                "scoring": report.scoring.name(),
                "rows": report.rows,
                "base_rate": report.base_rate,
                "brier_model": report.brier_model,
                "brier_base": report.brier_base,
                "log_loss_model": report.log_loss_model,
                "log_loss_base": report.log_loss_base,
                "deciles": report.deciles.iter().map(|decile| json!({"rows": decile.rows, "predicted": decile.mean_predicted, "observed": decile.observed_rate, "standard_error": decile.standard_error})).collect::<Vec<_>>(),
                "cells": report.cells.iter().map(|cell| json!({"label": cell.label, "weight": cell.weight, "events": cell.events, "posterior": cell.posterior})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"tickets": loaded.tickets, "skipped": loaded.skipped, "ledger_lines": loaded.ledger_lines, "reports": reports})
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let origin = match git(&options.repo, &["remote", "get-url", "--push", "origin"]) {
        Ok(origin) => origin,
        Err(error) => {
            eprintln!("north land model: {error}");
            return 1;
        }
    };
    let loaded = match load(&queue::directory(&state_home(), &origin), &options.repo, &options.base) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("north land model: {error}");
            return 1;
        }
    };
    let reports = reports(&loaded.lanes, options.era);
    if options.json {
        println!("{}", render_json(&loaded, &reports));
    } else {
        print!("{}", render(&loaded, &reports));
    }
    0
}

#[cfg(test)]
#[path = "model_cmd_tests.rs"]
mod tests;
