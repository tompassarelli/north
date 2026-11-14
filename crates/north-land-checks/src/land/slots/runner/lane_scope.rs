use north_base::foundation::exit::NO_VERDICT_STATUS;

use crate::land::slots::unit_reuse::quote;

pub(super) const FAILED: &str = "land lane failed: ";
const NO_VERDICT: &str = "land check reached no verdict (exit 75): it could not run, so no lane is blamed";

pub(super) fn wrapped(command: &str, lanes: &[String]) -> String {
    let each: Vec<String> = lanes.iter().map(|lane| quote(lane)).collect();
    format!(
        "rc=0\nfor lane in {}; do\n  range=${{lane##*=}}\n  (export SAFE_PUSH_LAND_BASE=${{range%%..*}} SAFE_PUSH_LAND_HEAD=${{range##*..}}; bash -euo pipefail -c {}) || {{ status=$?; if [ $status -eq {NO_VERDICT_STATUS} ]; then echo \"{NO_VERDICT}\"; [ $rc -eq 1 ] || rc={NO_VERDICT_STATUS}; else echo; echo \"{FAILED}${{lane%=*}}\"; rc=1; fi; }}\ndone\nexit $rc",
        each.join(" "),
        quote(command)
    )
}

pub(super) fn failed(log: &str) -> Vec<&str> {
    log.lines().filter_map(|line| line.strip_prefix(FAILED).map(str::trim)).collect()
}

#[cfg(test)]
#[path = "lane_scope_tests.rs"]
mod tests;
