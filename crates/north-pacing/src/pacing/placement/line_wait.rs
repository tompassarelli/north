//! A code-work launch waits while every build host's landing line is slower than `launch_line_wait_s`: the box it
//! starts would queue its builds and its landing behind that line. Each host answers `north capacity offload line wait --rank 2`,
//! which the survey keeper asks beside its survey answer so a launch reads a recorded reading instead of asking the host;
//! a host that does not answer counts as open. No `--pin`, `--local` or `--priority` passes the hold.
use super::reach::reaches;
use north_strategy_types::placement::LineWait;
use toml_edit::{DocumentMut, Item};

const SWITCH: &str = "launch_line_wait";
const BAR: &str = "launch_line_wait_s";
pub(super) const ASK: &str = ".local/state/north/current/tree/bin/north capacity offload line wait --rank 2 2>&1";

fn setting(name: &str) -> Option<i64> {
    crate::platform::harness::document(crate::platform::harness::DEFAULTS)[name]["value"].as_integer()
}

pub fn enabled() -> bool {
    setting(SWITCH) != Some(0)
}

pub fn bar_s() -> u64 {
    setting(BAR).and_then(|bar| u64::try_from(bar).ok()).unwrap_or(60)
}

pub fn parse(output: &str) -> Option<u64> {
    let (_, rest) = output.split_once("expected wait ")?;
    rest.split_whitespace().next()?.parse().ok()
}

/// Whether a launch of this role waits on the line: a worker's builds and landing queue there, a lead's or the coordinator's do not.
pub fn counts(role: Option<&str>) -> bool {
    matches!(role, None | Some("worker"))
}

fn lists(cfg: &DocumentMut, host: &str) -> Option<Vec<String>> {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(reaches)
}

/// The hosts a landing from `here` goes to: `here` itself and the hosts it reaches, or, when it reaches none, the host that settles
/// the fleet pulls its spool and lands for it, so that host and the hosts it reaches.
pub fn landing_hosts(cfg: &DocumentMut, here: &str) -> Vec<String> {
    let own = lists(cfg, here).filter(|reached| !reached.is_empty());
    let via_lander = || {
        let settles = |host: &Item| host.get("settles_fleet").and_then(Item::as_bool) == Some(true);
        let lander = cfg.get("hosts").and_then(Item::as_table_like).and_then(|hosts| hosts.iter().find(|(_, host)| settles(host)).map(|(name, _)| name.to_owned()));
        lander.map(|lander| [vec![lander.clone()], lists(cfg, &lander).unwrap_or_default()].concat()).unwrap_or_default()
    };
    std::iter::once(here.to_owned()).chain(own.unwrap_or_else(via_lander)).collect()
}

pub fn takes_builds(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("offload_builds")).and_then(Item::as_bool) == Some(true)
}

/// Why every build host's line is over the bar: the shortest wait among them, None while a host is unread or under it.
pub fn over_the_bar(waits: &[LineWait], bar_s: u64) -> Option<String> {
    let seconds: Vec<(&str, u64)> = waits.iter().map(|wait| wait.seconds.map(|seconds| (wait.host.as_str(), seconds))).collect::<Option<_>>()?;
    let (host, shortest) = seconds.iter().copied().min_by_key(|(_, seconds)| *seconds)?;
    (shortest > bar_s).then(|| format!("every build host's expected wait for a landing slot is over {bar_s} s (shortest {shortest} s on {host})"))
}
