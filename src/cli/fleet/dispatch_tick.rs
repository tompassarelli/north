use std::path::Path;

use toml_edit::DocumentMut;

const RECORD: &str = "dispatch-tick";
pub(super) const EVERY_SECS: f64 = 5.0 * 60.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Pass {
    Ran(u8),
    Recent,
    Paused(String),
    Elsewhere(String),
}

pub(super) fn pass(host: &str, cfg: Option<&DocumentMut>, paused: Option<&str>, directory: &Path, now: f64, dispatch: impl FnOnce() -> u8) -> Pass {
    if let Some(why) = elsewhere(host, cfg) {
        return Pass::Elsewhere(why);
    }
    if let Some(reason) = paused {
        return Pass::Paused(reason.to_owned());
    }
    if last(directory).is_some_and(|last| now - last < EVERY_SECS) {
        return Pass::Recent;
    }
    let recorded = std::fs::create_dir_all(directory).and_then(|()| std::fs::write(directory.join(RECORD), format!("{now:.0}")));
    if let Err(error) = recorded {
        eprintln!("north fleet tick: dispatch run not recorded in {}: {error}", directory.display());
    }
    Pass::Ran(dispatch())
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
