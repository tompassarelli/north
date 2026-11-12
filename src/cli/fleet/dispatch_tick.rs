use std::path::Path;
use std::time::{Duration, Instant};

use toml_edit::DocumentMut;

use crate::fleet::stage::detach;

const RECORD: &str = "dispatch-tick";
const RESIDENT: &str = "dispatch-resident";
pub(super) const EVERY_SECS: f64 = 5.0 * 60.0;
const FREED_AFTER_SECS: f64 = 20.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Pass {
    Ran(u8),
    Recent,
    Paused(String),
    Elsewhere(String),
}

pub(super) fn within(budget: Duration, run: impl FnOnce() -> Pass + Send + 'static) -> Result<Pass, String> {
    detach("dispatch", run).by(Instant::now() + budget).map_err(|late| format!("deferred dispatch to the next tick: {}", late.why(budget)))
}

pub(super) fn pass(host: &str, cfg: Option<&DocumentMut>, paused: Option<&str>, directory: &Path, now: f64, resident: impl Fn() -> Option<i64>, dispatch: impl FnOnce() -> u8) -> Pass {
    if let Some(why) = elsewhere(host, cfg) {
        return Pass::Elsewhere(why);
    }
    if let Some(reason) = paused {
        return Pass::Paused(reason.to_owned());
    }
    if last(directory).is_some_and(|last| now - last < EVERY_SECS && !freed(directory, now - last, &resident)) {
        return Pass::Recent;
    }
    let recorded = std::fs::create_dir_all(directory).and_then(|()| std::fs::write(directory.join(RECORD), format!("{now:.0}")));
    if let Err(error) = recorded {
        eprintln!("north fleet tick: dispatch run not recorded in {}: {error}", directory.display());
    }
    let code = dispatch();
    let _ = match resident() {
        Some(count) => std::fs::write(directory.join(RESIDENT), count.to_string()),
        None => std::fs::remove_file(directory.join(RESIDENT)),
    };
    Pass::Ran(code)
}

fn freed(directory: &Path, since: f64, resident: &impl Fn() -> Option<i64>) -> bool {
    if since < FREED_AFTER_SECS {
        return false;
    }
    let before: Option<i64> = std::fs::read_to_string(directory.join(RESIDENT)).ok().and_then(|text| text.trim().parse().ok());
    matches!((before, resident()), (Some(before), Some(now)) if now < before)
}

fn elsewhere(host: &str, cfg: Option<&DocumentMut>) -> Option<String> {
    let named = cfg.and_then(|cfg| cfg.get("placement")).and_then(|placement| placement.get("dispatch_host")).and_then(toml_edit::Item::as_str);
    match named {
        Some(named) if named == host => None,
        Some(named) => Some(format!("this host is not the dispatch host, {named}")),
        None => Some("no [placement] dispatch_host names the host that runs dispatch".into()),
    }
}

fn last(directory: &Path) -> Option<f64> {
    std::fs::read_to_string(directory.join(RECORD)).ok()?.trim().parse().ok()
}

#[cfg(test)]
#[path = "dispatch_tick_tests.rs"]
mod tests;
