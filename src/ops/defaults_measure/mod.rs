mod capacity;
mod priors;
mod proposal;
mod registry;
mod rollouts;
mod row;
mod sweeps;
mod training;
mod usage;
#[cfg(test)]
mod tests;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;
use registry::Registry;
use row::Row;

const USAGE: &str = "usage: north defaults-measure DB KIND [--since DATE] [--days N] [--state DIR]
       north defaults-measure --priors DB [--state DIR]
       north defaults-measure DB [KIND] --propose --train-until DATE [--since DATE] --json
       north defaults-measure DB --picks
Walk-forward measurements behind harness/defaults.toml: each KIND sweeps one default over a grid on a read-only copy
of the NorthThreads ledger and prints n and the score per value as JSON lines; --priors counts each prior's settling
data; --propose emits read-only proposals trained only on runs that ended before --train-until; --picks prints each
walk-forward recommend decision as {agent, pick, ok, cost_usd}.";

pub(super) const KINDS: [&str; 12] = [
    "box_calibration_n",
    "box_chain_gap_min",
    "calibration_thin_n",
    "capacity_high_confidence_runs",
    "capacity_min",
    "capacity_sizing_runs",
    "eta_coverage",
    "eta_min_samples",
    "incumbent_margin",
    "min_runs",
    "money_ceiling",
    "success_threshold",
];

#[derive(Debug, Default, PartialEq)]
struct Args {
    db: Option<PathBuf>,
    kind: Option<String>,
    priors: Option<PathBuf>,
    state: Option<PathBuf>,
    since: String,
    days: f64,
    propose: bool,
    picks: bool,
    train_until: Option<String>,
    json: bool,
}

pub(super) struct Context {
    registry: Registry,
    state: Option<PathBuf>,
    codex_root: PathBuf,
    days: f64,
    since: String,
}

impl Context {
    fn state_dir(&self) -> PathBuf {
        self.state.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(|| Path::new(&home).join(".local/state"), PathBuf::from).join("agents")
        })
    }

    fn capacity_episodes(&self, days: f64) -> rollouts::Episodes {
        let since = Timestamp::now().unix_seconds_f64() - days * 86400.0;
        rollouts::capacity_episodes(&rollouts::turns(&self.codex_root, since))
    }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let args = match parse(arguments) {
        Ok(args) => args,
        Err(message) => return refuse(&message),
    };
    let context = Context {
        registry: Registry::bundled(),
        state: args.state.clone(),
        codex_root: rollouts::codex_root(),
        days: args.days,
        since: args.since.clone(),
    };
    let mut out = io::stdout().lock();
    match measure(&args, &context, &mut out) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("north defaults-measure: {message}");
            1
        }
    }
}

fn refuse(message: &str) -> u8 {
    eprintln!("{USAGE}\nnorth defaults-measure: error: {message}");
    2
}

fn parse(arguments: &[String]) -> Result<Args, String> {
    let mut args = Args { since: "2026-10-05".to_owned(), days: 7.0, ..Args::default() };
    let mut positional = Vec::new();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--priors" => args.priors = Some(PathBuf::from(value("--priors")?)),
            "--state" => args.state = Some(PathBuf::from(value("--state")?)),
            "--since" => args.since = value("--since")?,
            "--days" => args.days = value("--days")?.parse().map_err(|_| "--days takes a number".to_owned())?,
            "--train-until" => args.train_until = Some(value("--train-until")?),
            "--propose" => args.propose = true,
            "--picks" => args.picks = true,
            "--json" => args.json = true,
            "-h" | "--help" => return Err("help".to_owned()),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => positional.push(argument.clone()),
        }
    }
    let mut positional = positional.into_iter();
    args.db = positional.next().map(PathBuf::from);
    args.kind = positional.next();
    if let Some(extra) = positional.next() {
        return Err(format!("unexpected argument {extra}"));
    }
    validate(args)
}

fn validate(mut args: Args) -> Result<Args, String> {
    if let Some(kind) = &args.kind
        && !KINDS.contains(&kind.as_str())
    {
        return Err(format!("kind {kind:?} is not one of {}", KINDS.join(", ")));
    }
    if args.picks {
        if args.db.is_none() || args.kind.is_some() || args.propose || args.priors.is_some() || args.json || args.train_until.is_some() {
            return Err("--picks takes only db".to_owned());
        }
    } else if args.propose {
        if args.db.is_none() || args.train_until.is_none() || !args.json || args.priors.is_some() {
            return Err("--propose requires db, --train-until and --json, without --priors".to_owned());
        }
        let stamp = |text: &str| training::iso_stamp(text).ok_or_else(|| "--train-until and --since must be ISO dates or timestamps".to_owned());
        let until = stamp(args.train_until.as_deref().unwrap_or_default())?;
        args.since = stamp(&args.since)?;
        if args.since >= until {
            return Err("--since must precede --train-until".to_owned());
        }
        args.train_until = Some(until);
    } else if args.train_until.is_some() || args.json {
        return Err("--train-until and --json require --propose".to_owned());
    } else if args.priors.is_none() && (args.db.is_none() || args.kind.is_none()) {
        return Err("db and kind are required without --priors".to_owned());
    }
    Ok(args)
}

fn measure(args: &Args, context: &Context, out: &mut impl Write) -> Result<(), String> {
    let write = |out: &mut dyn Write, line: String| writeln!(out, "{line}").map_err(|error| error.to_string());
    if args.propose {
        let db = training::training_db(args.db.as_deref().unwrap_or_else(|| Path::new("")), args.train_until.as_deref().unwrap_or_default())?;
        for row in proposal::rows(&db, context, args.kind.as_deref(), args.train_until.as_deref().unwrap_or_default())? {
            write(out, row.to_json())?;
        }
        return Ok(());
    }
    let open = |path: &Path| training::open_read_only(path).map_err(|error| format!("{}: {error}", path.display()));
    if let Some(path) = &args.priors {
        for line in priors::priors(&open(path)?, context)? {
            write(out, line)?;
        }
        return Ok(());
    }
    let db = open(args.db.as_deref().unwrap_or_else(|| Path::new("")))?;
    if args.picks {
        for row in sweeps::picks(&db, &context.registry).map_err(|error| format!("picks: {error}"))? {
            write(out, row.to_json())?;
        }
        return Ok(());
    }
    for row in sweep(&db, context, args.kind.as_deref().unwrap_or_default())? {
        write(out, row.to_rounded_json())?;
    }
    Ok(())
}

fn sweep(db: &Connection, context: &Context, kind: &str) -> Result<Vec<Row>, String> {
    let sql = |error: rusqlite::Error| format!("{kind}: {error}");
    let defaults = &context.registry;
    let since = context.since.as_str();
    match kind {
        "success_threshold" | "min_runs" | "incumbent_margin" => sweeps::sweep_tier(db, defaults, since, kind, None).map_err(sql),
        "eta_min_samples" => sweeps::sweep_eta_min_samples(db, defaults, since, None).map_err(sql),
        "eta_coverage" => sweeps::sweep_eta_coverage(db, defaults, since, None).map_err(sql),
        "calibration_thin_n" => sweeps::sweep_thin_n(db, defaults, since).map_err(sql),
        "box_chain_gap_min" => sweeps::sweep_box_gap(db, defaults).map_err(sql),
        "box_calibration_n" => sweeps::sweep_box_n(db, defaults, since).map_err(sql),
        "capacity_min" => Ok(context.capacity_episodes(context.days).rows),
        "capacity_sizing_runs" => capacity::sweep_capacity_sizing(db, defaults.number("capacity_memory_margin")).map_err(sql),
        "capacity_high_confidence_runs" => {
            capacity::sweep_capacity_confidence(db, defaults.number("capacity_memory_margin"), defaults.number("capacity_sizing_runs") as usize).map_err(sql)
        }
        "money_ceiling" => usage::sweep_money_ceiling(db, defaults.number("usage_stale_min")).map_err(sql),
        other => Err(format!("kind {other:?} has no sweep")),
    }
}
