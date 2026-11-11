use crate::platform::git::run_within;
use std::path::Path;
use std::time::Duration;

const BOUND_SECONDS: u64 = 15;
const LIGHT_SUITE_FILES: usize = 50;
const TEST_MARKS: &[&str] = &[".test.", "_test.", ".spec.", "_spec."];

pub(super) const NOTE: &str = "north-hook: a full `bun test` of this suite of at most 50 test files runs here under a 15 s bound instead of capacity admission; past the bound it stops and names the admitted form.";

pub(super) fn light_suite(cwd: &Path) -> bool {
    let Some(directory) = cwd.to_str() else {
        return false;
    };
    let Some(listed) = run_within(&["-C", directory, "ls-files", "-z"], Duration::from_secs(2))
        .filter(|output| output.status.success())
    else {
        return false;
    };
    let tests = listed
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|path| std::str::from_utf8(path).ok())
        .filter(|path| test_file(path))
        .count();
    tests <= LIGHT_SUITE_FILES
}

fn test_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    TEST_MARKS.iter().any(|mark| name.contains(mark))
        && super::script_file(name)
        && !path.split('/').any(|part| part == "node_modules")
}

pub(super) fn wrapped(quoted: &str) -> String {
    format!(
        "timeout -k 5 {BOUND_SECONDS} bash -c {quoted}; north_status=$?; [ $north_status -ne 124 ] || echo 'north-hook: the full bun test passed its {BOUND_SECONDS} s bound outside capacity admission and was stopped; run it admitted with north capacity host run --owner OWNER --timeout-seconds 1200 -- bun test, or name its test files.' >&2; (exit $north_status)"
    )
}
