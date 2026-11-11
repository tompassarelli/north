use super::super::argument_words::{is_plain_argument, is_redirect_operator};
use super::{Decision, refuse};
use crate::platform::git::run_within;
use std::path::Path;
use std::time::Duration;

pub(super) fn bun_command(args: &[String], plain: &[&str], cwd: &Path) -> Decision {
    let runner = plain.first() == Some(&"test");
    let plain = plain.strip_prefix(&["run"]).unwrap_or(plain);
    if plain.first() == Some(&"test") {
        return bun_tests(args, runner.then_some(cwd));
    }
    if plain.starts_with(&["wisp", "map", "build"]) {
        return Decision::Route;
    }
    if plain.starts_with(&["wisp", "view"])
        || (plain.starts_with(&["wisp", "headless"]) && args.iter().any(|s| s == "--render"))
    {
        return Decision::Route;
    }
    if plain.first() == Some(&"tsc") && args.iter().any(|s| matches!(s.as_str(), "--build" | "-b"))
    {
        return Decision::Route;
    }
    Decision::Allow
}

fn bun_tests(args: &[String], runner: Option<&Path>) -> Decision {
    let targets = test_targets(args);
    match targets.as_slice() {
        [] if runner.is_some_and(light_suite) => Decision::Bound,
        [] => refuse("bun test (full suite)"),
        [file] if !file.contains(['*', '?', '[']) && script_file(file) => Decision::Allow,
        _ => Decision::Route,
    }
}

pub(super) fn script_file(path: &str) -> bool {
    [".ts", ".js", ".tsx", ".jsx", ".mjs", ".cjs", ".mts", ".cts"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

pub(super) fn test_targets(args: &[String]) -> Vec<&str> {
    let mut skip = false;
    args.iter()
        .skip_while(|arg| *arg != "test")
        .skip(1)
        .filter(|arg| {
            if skip {
                skip = false;
                return false;
            }
            if matches!(
                arg.as_str(),
                "--timeout"
                    | "--rerun-each"
                    | "--retry"
                    | "--seed"
                    | "--test-name-pattern"
                    | "-t"
                    | "--reporter"
                    | "--reporter-outfile"
                    | "--preload"
                    | "--coverage-dir"
                    | "--max-concurrency"
            ) {
                skip = true;
                return false;
            }
            if is_redirect_operator(arg) {
                skip = true;
                return false;
            }
            is_plain_argument(arg)
        })
        .map(String::as_str)
        .collect()
}

const BOUND_SECONDS: u64 = 15;
const LIGHT_SUITE_FILES: usize = 50;
const TEST_MARKS: &[&str] = &[".test.", "_test.", ".spec.", "_spec."];

pub(super) const NOTE: &str = "north-hook: a full `bun test` of this suite of at most 50 test files runs here under a 15 s bound instead of capacity admission; past the bound it stops and names the admitted form.";

fn light_suite(cwd: &Path) -> bool {
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
        && script_file(name)
        && !path.split('/').any(|part| part == "node_modules")
}

pub(super) fn wrapped(quoted: &str) -> String {
    format!(
        "timeout -k 5 {BOUND_SECONDS} bash -c {quoted}; north_status=$?; [ $north_status -ne 124 ] || echo 'north-hook: the full bun test passed its {BOUND_SECONDS} s bound outside capacity admission and was stopped; run it admitted with north capacity host run --owner OWNER --timeout-seconds 1200 -- bun test, or name its test files.' >&2; (exit $north_status)"
    )
}
