use crate::land::slots::unit_reuse::quote;

pub(super) const FAILED: &str = "land lane failed: ";

pub(super) fn wrapped(command: &str, lanes: &[String]) -> String {
    let each: Vec<String> = lanes.iter().map(|lane| quote(lane)).collect();
    format!(
        "rc=0\nfor lane in {}; do\n  range=${{lane##*=}}\n  (export SAFE_PUSH_LAND_BASE=${{range%%..*}} SAFE_PUSH_LAND_HEAD=${{range##*..}}; bash -euo pipefail -c {}) || {{ echo \"{FAILED}${{lane%=*}}\"; rc=1; }}\ndone\nexit $rc",
        each.join(" "),
        quote(command)
    )
}

pub(super) fn failed(log: &str) -> Vec<&str> {
    log.lines().filter_map(|line| line.split_once(FAILED).map(|(_, lane)| lane.trim())).collect()
}
