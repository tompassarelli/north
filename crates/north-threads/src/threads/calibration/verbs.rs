//! `north threads recommend` and `calibrate`: the calibration core over the ledger.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use super::mode::{self, Mode};
use super::read::{escalated, raw_runs};
use super::recommend::{Eta, asked_of, expected_line, recommend, recommend_eta, recommend_expected, recommend_usd};
use super::{Knobs, Pick, median, provider, scaled, tier_rank};
use crate::foundation::clock::Timestamp;
use crate::router::brief::CATEGORIES;
use crate::threads::runs::brief::Fields;
use crate::threads::verb::{Args, Context, Failure, Outcome, immediate, open, report};

const REVIEW: &str = "review";

pub const VERBS: [&str; 9] = ["recommend", "eta-calibration", "calibrate", "summary", "routing-compare", "box-eta", "annotate-runs", "recommend-runs", "calibration-runs"];

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv[0].as_str() {
        "summary" => return super::summary::run(&argv[1..], context, out, err),
        "routing-compare" => return super::routing::run(context, out, err),
        "box-eta" => super::box_eta::run(context, stdin),
        "eta-calibration" => super::eta_calibration::run(&argv[1..], context),
        "annotate-runs" => super::feeds::annotate(stdin),
        "recommend-runs" => super::feeds::recommend_cases(stdin),
        "calibration-runs" => super::feeds::runs(context),
        "recommend" => recommend_verb(&argv[1..], context),
        "calibrate" => super::calibrate::run(&argv[1..], context),
        other => Err(Failure::Usage(format!("unknown verb {other}"))),
    };
    report(&argv[0], result, out, err)
}

/// Python's `repr` of a string.
fn quoted(text: &str) -> String {
    if text.contains('\'') && !text.contains('"') { format!("\"{text}\"") } else { format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n")) }
}

pub(super) fn stat_usd(usd: Option<f64>) -> String {
    usd.map_or_else(|| "$ n/a".to_owned(), |usd| format!("${usd:.2}/success"))
}

/// `Nd` or `Nh` before `end` as a stamp, any other text as is.
pub(super) fn since_start(text: &str, end: Timestamp) -> String {
    let (digits, unit) = text.split_at(text.len().saturating_sub(1));
    match (digits.parse::<i64>(), unit) {
        (Ok(count), "d") if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => Timestamp::from_unix_seconds(end.unix_seconds() - count * 86_400).iso(),
        (Ok(count), "h") if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => Timestamp::from_unix_seconds(end.unix_seconds() - count * 3_600).iso(),
        _ => text.to_owned(),
    }
}

/// The newest calibration row of `kind` for the tier's provider, else for all: (scale, label), or no scale with why.
pub(super) fn latest_calibration(db: &Connection, knobs: &Knobs, now: Timestamp, kind: &str, tier: &str, median_scale: bool) -> Result<(Option<f64>, String), Failure> {
    let mut reason = format!("no {kind} calibration");
    for cell in [provider(tier), "all".to_owned()] {
        let row: Option<(String, i64, Option<f64>, Option<f64>)> = db
            .query_row("SELECT at, n, median, value FROM calibration WHERE kind = ?1 AND cell = ?2 ORDER BY rowid DESC LIMIT 1", params![kind, cell], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .map(Some)
            .or_else(|error| if error == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(error) })?;
        let Some((at, n, median, value)) = row else { continue };
        let value = if median_scale { median } else { value };
        let age = (now.unix_seconds() - Timestamp::parse_loose(&at).map_or(now.unix_seconds(), Timestamp::unix_seconds)).div_euclid(60) as f64 / 1440.0;
        let floor = if kind == "box" { knobs.box_calibration_n } else { knobs.calibration_thin_n };
        match value {
            Some(value) if n as usize >= floor && age <= knobs.calibration_stale_days => {
                let note = if median_scale { "median of actual/predicted".to_owned() } else { format!("median {:.2}", median.unwrap_or(0.0)) };
                return Ok((Some(value), format!("x{value:.2} {kind} {cell} n={n} {note}")));
            }
            _ if (n as usize) < floor || value.is_none() => reason = format!("{kind} {cell} n={n} < {floor}"),
            _ => reason = format!("{kind} {cell} {age:.0}d old"),
        }
    }
    Ok((None, format!("uncalibrated: {reason}")))
}

/// The walk-forward `eta_pred` scale a brief's ETA is multiplied by for `tier`, read from the ledger at `database`,
/// with its label; no scale when the newest row is thin, stale or missing.
pub fn eta_pred_scale(database: &Path, now: Timestamp, tier: &str) -> Result<(Option<f64>, String), String> {
    let refused = |failure: Failure| match failure {
        Failure::Usage(why) | Failure::Refused(why) => why,
    };
    let db = open(database, false).map_err(refused)?;
    latest_calibration(&db, &Knobs::registry(), now, "eta_pred", tier, false).map_err(refused)
}

fn recommend_verb(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--since"], &[])?;
    let [facets] = args.positional.as_slice() else { return Err(Failure::Usage("usage: threads recommend [--since DATE] FACETS".into())) };
    let fields = Fields::read(facets);
    if fields.category.as_deref() == Some(REVIEW) {
        return crate::threads::review_findings::recommend(context, args.get("--since").unwrap_or(""));
    }
    if fields.known_category().is_none() {
        return Err(Failure::Refused(format!("recommend needs `Category: <one of {}>` in {}", CATEGORIES.join(", "), quoted(facets))));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    recommend_with(&args, context, &fields, &mode::configuration(&home))
}

pub(super) fn recommend_with(args: &Args, context: &Context, fields: &Fields, configuration: &Path) -> Outcome {
    let mode = mode::resolve(configuration)?;
    let knobs = Knobs::registry();
    let db = open(context.database, false)?;
    let escalated = escalated(&db)?;
    let since = args.get("--since").unwrap_or("");
    let mut runs: Vec<_> = raw_runs(&db, "follows IS NULL AND outcome != 'split' AND ended >= ?1", &[&since])?
        .into_iter()
        .filter(|raw| !raw.unjudged())
        .map(|raw| raw.recommend_run(raw.succeeded(escalated.contains(&raw.agent))))
        .collect();
    let codex_only = mode.name == "codex-only";
    if codex_only {
        runs.retain(|run| mode.allowed.contains(&run.tier));
    }
    let incumbent: BTreeMap<String, String> = db.prepare("SELECT cell, tier FROM tier_choice")?.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    let asked = asked_of(fields);
    let pick = recommend(&knobs, &runs, &asked, &incumbent, true);
    drop(db);
    if let Some(tier) = pick.tier.as_deref().filter(|tier| incumbent.get(&pick.cell).map(String::as_str) != Some(*tier)).filter(|_| !context.replica) {
        let mut db = open(context.database, true)?;
        let tx = immediate(&mut db)?;
        tx.execute("INSERT INTO tier_choice(cell, tier, since) VALUES (?1,?2,?3) ON CONFLICT(cell) DO UPDATE SET tier = excluded.tier, since = excluded.since", params![pick.cell, tier, context.now.iso()])?;
        tx.commit()?;
    }
    let db = open(context.database, false)?;
    report_pick(&db, &knobs, context, &mode, &runs, &pick, fields)
}

fn report_pick(db: &Connection, knobs: &Knobs, context: &Context, mode: &Mode, runs: &[super::Run], pick: &Pick, fields: &Fields) -> Outcome {
    let mut text = pick_report(db, knobs, context, mode, runs, pick, fields.known_category().unwrap_or_default())?;
    if let Some(tier) = pick.tier.as_deref() {
        let by_ref: Vec<&super::Run> = runs.iter().collect();
        text += &expected_line(recommend_expected(knobs, &by_ref, &asked_of(fields), tier).as_ref(), tier, knobs.min_runs);
        text.push('\n');
    }
    Ok(text)
}

fn pick_report(db: &Connection, knobs: &Knobs, context: &Context, mode: &Mode, runs: &[super::Run], pick: &Pick, category: &str) -> Outcome {
    let mut text = String::new();
    match &pick.tier {
        Some(tier) => text += &format!("tier {tier}  cell [{}]  n {}\n", pick.cell, pick.n),
        None => text += &format!("insufficient evidence  cell [{}]  n {}\n", pick.cell, pick.n),
    }
    text += &format!("success threshold {}%  rule {}\n", knobs.success_threshold, pick.rule);
    if let Some((tier, stat)) = pick.tier.as_deref().and_then(|tier| pick.stats.iter().find(|(key, _)| key == tier)) {
        text += &format!("success {tier} {:.0}% shrunk (n={}, {} ok, toward the category's pooled {:.0}% with a prior of {} runs)\n", stat.shrunk * 100.0, stat.n, stat.ok, pick.pooled * 100.0, crate::router::policy::ARM_RUNS);
    } else if let Some(tier) = &pick.tier {
        text += &format!("success {tier} unmeasured in this cell (n=0)\n");
    }
    let mut stats = pick.stats.clone();
    stats.sort_by_key(|(tier, _)| tier_rank(tier));
    for (tier, stat) in stats {
        text += &format!("  {tier:<18} {}% of {}  shrunk {:.0}%  {} (priced n={})\n", stat.rate, stat.n, stat.shrunk * 100.0, stat_usd(stat.usd), stat.usd_n);
    }
    let Some(tier) = pick.tier.as_deref() else { return Ok(text) };
    let by_ref: Vec<&super::Run> = runs.iter().collect();
    let (usd, usd_n) = recommend_usd(&by_ref, category, tier);
    let (usd_scale, usd_label) = latest_calibration(db, knobs, context.now, "usd", tier, false)?;
    let cost = match usd.filter(|usd| *usd != 0.0) {
        Some(usd) => format!("  ${:.2} (median of n={usd_n}, {usd_label})", scaled(usd, usd_scale, false)),
        None => format!("  $ n/a ({usd_label})"),
    };
    if mode.name == "codex-only" {
        let fields = Fields::read(&pick.cell);
        let asked = asked_of(&fields);
        let samples: Vec<f64> = runs
            .iter()
            .filter(|run| run.tier == tier && asked.iter().zip(&run.cells).all(|(want, have)| want.as_ref().is_none_or(|want| want == have)))
            .filter_map(|run| run.minutes)
            .collect();
        match median(&samples).filter(|_| samples.len() >= knobs.min_runs) {
            Some(eta) => text += &format!("eta {eta}m (actual_med of n={}, cell {}, tier {tier}){cost}\n", samples.len(), pick.cell),
            None => text += &format!("eta n/a (n={}){cost}\n", samples.len()),
        }
        return Ok(text);
    }
    let eta = recommend_eta(knobs, &by_ref, &pick.cell, tier);
    let (scale, label) = latest_calibration(db, knobs, context.now, "eta_pred", tier, false)?;
    let Some(wall) = eta.wall_p75.filter(|_| eta.wall_n > 0) else {
        text += &format!("eta n/a (n=0){cost}\n");
        return Ok(text);
    };
    text += &eta_line(&eta, scaled(wall, scale, true), tier, &label);
    text += &cost;
    text.push('\n');
    Ok(text)
}

/// The recommend verb's eta line: the active-minute spread, wall p75 and median wait when enough runs recorded active
/// minutes, else the wall p75 alone labelled as such.
pub(super) fn eta_line(eta: &Eta, wall: f64, tier: &str, label: &str) -> String {
    let place = format!("cell {}, tier {tier}, {label}", eta.cell);
    match &eta.active {
        Some(active) => {
            let wait = eta.wait_median.map_or_else(String::new, |wait| format!(" wait median {wait:.0}m"));
            format!("eta active {:.0}m (p25 {:.0}\u{2013}p90 {:.0}, n={}) wall p75 {wall}m{wait} ({place}, wall n={})", active.median, active.p25, active.p90, active.n, eta.wall_n)
        }
        None => format!("eta wall {wall}m (p75 of n={}, {place}; too few runs with active minutes)", eta.wall_n),
    }
}
