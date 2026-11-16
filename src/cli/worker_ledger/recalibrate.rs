use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;

use toml_edit::DocumentMut;

use crate::foundation::clock::Timestamp;

static DEFAULTS: std::sync::LazyLock<&str> = std::sync::LazyLock::new(|| crate::platform::harness::table(crate::platform::harness::DEFAULTS));
const GOAL_KINDS: [&str; 2] = ["goal_eta", "goal_actual"];

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub(super) fn owns_ledger() -> bool {
    !home().join(".config/threads/remote").exists()
}

fn explicit_ledger() -> Option<OsString> {
    std::env::var_os("THREADS_DB").filter(|path| !path.is_empty())
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
}
