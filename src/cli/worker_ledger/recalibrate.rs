use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use toml_edit::DocumentMut;

use crate::foundation::clock::Timestamp;

const DEFAULTS: &str = include_str!("../../../harness/defaults.toml");
const GOAL_KINDS: [&str; 2] = ["goal_eta", "goal_actual"];
const DAY: i64 = 86_400;
const MONDAY: i64 = 4;

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub(super) fn owns_ledger() -> bool {
    !home().join(".config/threads/remote").exists()
}

fn explicit_ledger() -> Option<OsString> {
    std::env::var_os("THREADS_DB").filter(|path| !path.is_empty())
}

fn ledger_path() -> PathBuf {
    crate::usage::ledger_path(&home(), explicit_ledger())
}

fn calibration_rows() -> Result<Vec<(i64, String)>, String> {
    let db = crate::usage::open_ledger(&home(), explicit_ledger(), &crate::threads::client::refresh_mirrored_snapshot)?;
    let fail = |error: rusqlite::Error| error.to_string();
    let mut query = db.prepare("SELECT at, kind FROM calibration").map_err(fail)?;
    let found = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(fail)?;
    let rows = found.collect::<Result<Vec<_>, _>>().map_err(fail)?;
    Ok(rows.into_iter().filter_map(|(at, kind)| Some((Timestamp::parse_loose(&at)?.unix_seconds(), kind))).collect())
}

fn newest(rows: &[(i64, String)], keep: impl Fn(&str) -> bool) -> Option<i64> {
    rows.iter().filter(|(_, kind)| !GOAL_KINDS.contains(&kind.as_str()) && keep(kind)).map(|(at, _)| *at).max()
}

fn calibrate_after_seconds() -> i64 {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    let minutes = &defaults["calibrate_after_min"]["value"];
    minutes.as_integer().or_else(|| minutes.as_float().map(|value| value as i64)).expect("harness/defaults.toml has calibrate_after_min") * 60
}

fn calibration_due(rows: &[(i64, String)], now: i64) -> bool {
    newest(rows, |kind| kind != "prior").is_none_or(|at| now - at > calibrate_after_seconds())
}

fn priors_due(rows: &[(i64, String)], now: i64) -> bool {
    let days = now.div_euclid(DAY);
    let week = (days - (days - MONDAY).rem_euclid(7)) * DAY;
    newest(rows, |kind| kind == "prior").is_none_or(|at| at < week)
}

fn codex_root() -> PathBuf {
    std::env::var_os("CODEX_CONFIG_DIR").map_or_else(|| home().join("code/north-data/codex-pooled"), PathBuf::from)
}

fn prior_needs() -> Vec<(String, i64)> {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    defaults
        .iter()
        .filter(|(_, entry)| entry.get("source").and_then(|source| source.as_str()).is_some_and(|source| source.starts_with("prior: ")))
        .filter_map(|(name, entry)| Some((name.to_owned(), entry.get("settles_when")?.get("need")?.as_integer()?)))
        .collect()
}

fn prior_specs(printed: &str, needs: &[(String, i64)]) -> Vec<String> {
    let mut specs = Vec::new();
    for line in printed.lines() {
        let mut words = line.split_whitespace();
        let (Some(state), Some(name), Some(count)) = (words.next(), words.next(), words.next()) else { continue };
        let Some((_, need)) = needs.iter().find(|(known, _)| known == name) else { continue };
        let Some(count) = count.strip_prefix("n=") else { continue };
        if !matches!(state, "SETTLED" | "WAIT") {
            continue;
        }
        let value = words.find_map(|word| word.strip_prefix("value=")).filter(|value| !value.is_empty());
        let count = count.split('/').next().unwrap_or("");
        specs.push(format!("{name}:{count}:{need}{}", value.map(|value| format!(":{value}")).unwrap_or_default()));
    }
    specs
}

const MEASURE_LIMIT: Duration = Duration::from_secs(600);

fn exit_code(status: std::io::Result<std::process::ExitStatus>) -> i32 {
    status.ok().and_then(|status| status.code()).unwrap_or(1)
}

fn calibrate() {
    let code = match super::north() {
        Ok(mut north) => exit_code(north.args(["threads", "calibrate", "--write"]).stdout(Stdio::null()).status()),
        Err(error) => {
            eprintln!("recalibrate: cannot find the running north: {error}");
            1
        }
    };
    eprintln!("recalibrate: threads calibrate --write rc={code}");
}

fn priors() {
    let command = super::north().map(|mut north| {
        north.args(["defaults-measure", "--priors"]).arg(ledger_path());
        north
    });
    let measured = command.map_err(|error| format!("cannot find the running north: {error}")).and_then(|command| {
        crate::platform::process::run_command_within(command, None, MEASURE_LIMIT).ok_or_else(|| format!("defaults-measure --priors did not finish within {} s", MEASURE_LIMIT.as_secs()))
    });
    let (mut code, mut note, mut specs) = (1, String::new(), Vec::new());
    match &measured {
        Ok(done) => {
            code = done.status.code().unwrap_or(1);
            let stderr = String::from_utf8_lossy(&done.stderr).trim().to_owned();
            note = stderr.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            if done.status.success() {
                specs = prior_specs(&String::from_utf8_lossy(&done.stdout), &prior_needs());
            }
        }
        Err(why) => note.clone_from(why),
    }
    if !specs.is_empty() {
        code = match super::north() {
            Ok(mut north) => exit_code(north.args(["threads", "record-priors"]).args(&specs).status()),
            Err(_) => 1,
        };
    }
    let shown = if specs.is_empty() { note } else { specs.join(" ") };
    eprintln!("recalibrate: priors {shown} rc={code}");
}

pub(super) fn run() {
    let rows = match calibration_rows() {
        Ok(rows) => rows,
        Err(why) => {
            eprintln!("recalibrate: calibration rows unreadable: {why}");
            return;
        }
    };
    let now = Timestamp::now().unix_seconds();
    if owns_ledger() && calibration_due(&rows, now) {
        calibrate();
    }
    if codex_root().is_dir() && priors_due(&rows, now) {
        priors();
    }
}
