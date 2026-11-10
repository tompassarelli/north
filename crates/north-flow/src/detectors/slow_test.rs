use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub use north_graph_loops::slow_test::{Slow as Crossing, read as told, tell};

use regex::Regex;

use super::{Detector, Finding, Limits};

static PASSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:PASS|LEAK) \[\s*(\d+(?:\.\d+)?)s\]\s+\(\s*\d+/\d+\)\s+(\S+)\s+(\S+)\s*$").expect("nextest pass pattern"));
static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d+(?:\.\d+)?)\s+(\d+\.\d+)\s+(?:\d+\s+)?(\d+)(?:\s+\d+)?\s+(\S+)\s*$").expect("strace row pattern"));
const SUMMARY_ROWS: usize = 5;
pub const PROFILE_MARK: &str = "== strace -c summary ==";

pub type Run = BTreeMap<(String, String), f64>;

pub fn check_log(name: &str, finished: bool) -> bool {
    let Some(stem) = name.strip_suffix(".log") else { return false };
    match stem.strip_prefix("post-land-") {
        Some(sha) => finished && !sha.contains("-on-"),
        None => stem.ends_with("-candidate") || stem.rsplit_once("-speculative-").is_some_and(|(_, level)| level.chars().all(|digit| digit.is_ascii_digit())),
    }
}

pub fn passed(log: &str) -> Run {
    let mut times = Run::new();
    for found in PASSED.captures_iter(log) {
        let seconds: f64 = found[1].parse().unwrap_or_default();
        let slot = times.entry((found[2].to_owned(), found[3].to_owned())).or_default();
        *slot = f64::max(*slot, seconds);
    }
    times
}


pub fn crossing(runs: &[Run], limits: &Limits) -> Vec<Crossing> {
    let mut samples: BTreeMap<(&str, &str), Vec<f64>> = BTreeMap::new();
    for run in runs {
        for ((binary, name), seconds) in run {
            samples.entry((binary.as_str(), name.as_str())).or_default().push(*seconds);
        }
    }
    samples
        .into_iter()
        .filter_map(|((binary, name), seconds)| {
            let newest = seconds.get(seconds.len().checked_sub(limits.slow_test_runs)?..)?;
            newest.iter().all(|seconds| *seconds > limits.slow_test_seconds).then(|| Crossing { binary: binary.to_owned(), name: name.to_owned(), seconds: *newest.last().expect("slow_test_runs newest runs, at least one") })
        })
        .collect()
}

pub struct Slow {
    pub crossing: Crossing,
    pub file: String,
    pub owner: String,
    pub profile: String,
}

#[derive(Default)]
pub struct Facts {
    pub slow: Vec<Slow>,
}

pub fn key(binary: &str, name: &str) -> String {
    format!("slow-test:{binary}:{name}")
}

pub fn slow(facts: &Facts, limits: &Limits) -> Vec<Finding> {
    let mut findings: Vec<Finding> = facts
        .slow
        .iter()
        .map(|slow| {
            let Crossing { binary, name, seconds } = &slow.crossing;
            Finding {
                detector: Detector::SlowTest,
                key: key(binary, name),
                owner: Some(slow.owner.clone()),
                title: format!("Speed up the slow test {name}"),
                detail: format!("`{binary} {name}` in {} took {seconds:.1} s in its newest land check run and was over the {:.0} s line in each of its newest {} runs; {}", slow.file, limits.slow_test_seconds, limits.slow_test_runs, slow.profile),
                done_when: format!("`north ledger flow slow-tests --test {}` exits 0.", key(binary, name)),
                instances: vec![format!("{binary} {name}")],
            }
        })
        .collect();
    findings.sort_by(|a, b| a.key.cmp(&b.key));
    findings
}

pub fn summarize(output: &str) -> Option<String> {
    let (_, table) = output.split_once(PROFILE_MARK)?;
    let rows: Vec<(f64, f64, u64, &str)> = table
        .lines()
        .filter_map(|line| {
            let found = ROW.captures(line)?;
            Some((found[1].parse().ok()?, found[2].parse().ok()?, found[3].parse().ok()?, found.get(4)?.as_str()))
        })
        .collect();
    let total = rows.iter().find(|row| row.3 == "total")?;
    let top: Vec<String> = rows.iter().filter(|row| row.3 != "total").take(SUMMARY_ROWS).map(|(share, seconds, calls, syscall)| format!("{syscall} {share:.1}% {seconds:.2} s in {calls} calls")).collect();
    (!top.is_empty()).then(|| format!("`strace -c -w -f` of the one test on the build host, {:.2} s of syscall wall time in all, top syscalls: {}.", total.1, top.join("; ")))
}

fn word(text: &str, extra: &str) -> bool {
    !text.is_empty() && text.chars().all(|character| character.is_ascii_alphanumeric() || character == '_' || extra.contains(character))
}

pub fn profile_command(binary: &str, name: &str) -> Option<String> {
    let (package, target) = binary.split_once("::").map_or((binary, None), |(package, target)| (package, Some(target)));
    let select = match target {
        None => "--lib".to_owned(),
        Some(bin) if bin.starts_with("bin/") && word(&bin[4..], "-") => format!("--bin {}", &bin[4..]),
        Some(test) if word(test, "-") => format!("--test {test}"),
        Some(_) => return None,
    };
    (word(package, "-") && word(name, ":")).then(|| {
        format!(
            "p=\"$PWD/target/slow-profile-$$.txt\"; CARGO_TARGET_DIR=target ./bin/land-shell --dev -- env CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=\"strace -c -w -f -o $p\" cargo test --locked --offline -p {package} {select} -- --exact {name} >&2; echo '{PROFILE_MARK}'; cat \"$p\"; rm -f \"$p\""
        )
    })
}

pub fn pick(files: &[&str], binary: &str, name: &str) -> Option<String> {
    let package = binary.split("::").next().unwrap_or(binary);
    let own = format!("crates/{package}/");
    let files = files.iter().filter(|file| if package == "north" { !file.starts_with("crates/") } else { file.starts_with(&own) });
    let segments: Vec<&str> = binary.split("::").skip(1).chain(name.split("::")).collect();
    let score = |file: &str| segments.iter().filter(|segment| file.split(['/', '.']).any(|part| part == **segment || part.strip_suffix("_tests") == Some(**segment))).count();
    files.max_by_key(|file| (score(file), std::cmp::Reverse(file.len()))).map(|file| (*file).to_owned())
}

/// The newest land check runs that name one test, oldest first, or how many were found when fewer than asked.
#[derive(Clone, Debug, PartialEq)]
pub enum Newest {
    Runs(Vec<f64>),
    Missing { found: usize, want: usize },
}

/// Every finished land check log under `state/north/land-queue`, oldest first: the logs the slow-test
/// detector reads, so a box's Done when and the finding that filed it read the same runs.
fn check_logs(state: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(state.join("north/land-queue"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|queue| fs::read_dir(queue.path().join("logs")).into_iter().flatten().flatten())
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| check_log(name, entry.path().with_extension("outcome").is_file())))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    logs.sort_by_key(|entry| entry.0);
    logs.into_iter().map(|(_, path)| path).collect()
}

/// The times the test behind `key` (`slow-test:BINARY:NAME`) passed in its newest `runs` land check logs,
/// oldest first; a log that does not name the test is no run of it, since selection may have skipped it.
pub fn newest(state: &Path, key: &str, runs: usize) -> Newest {
    let mut times: Vec<f64> = Vec::new();
    for log in check_logs(state).iter().rev() {
        let text = fs::read_to_string(log).unwrap_or_default();
        let Some(seconds) = passed(&text).into_iter().find(|((binary, name), _)| self::key(binary, name) == key).map(|(_, seconds)| seconds) else { continue };
        times.push(seconds);
        if times.len() == runs {
            break;
        }
    }
    times.reverse();
    if times.len() < runs { Newest::Missing { found: times.len(), want: runs } } else { Newest::Runs(times) }
}

pub struct Document {
    pub owner: String,
    pub paths: Vec<String>,
}

static OWNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Owner: (\S+#\d+)").expect("owner pattern"));
static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([A-Za-z0-9_./-]+/[A-Za-z0-9_./-]*)`").expect("path pattern"));

pub fn document(text: &str) -> Option<Document> {
    let owner = OWNER.captures(text)?[1].to_owned();
    Some(Document { owner, paths: PATH.captures_iter(text).map(|found| found[1].to_owned()).collect() })
}

pub fn owner_of(file: &str, documents: &[Document], fallback: &str) -> String {
    let reach = |path: &String| (file == path || file.starts_with(&format!("{}/", path.trim_end_matches('/')))).then_some(path.len());
    documents
        .iter()
        .filter_map(|document| Some((document.paths.iter().filter_map(reach).max()?, &document.owner)))
        .max_by_key(|(length, _)| *length)
        .map_or_else(|| fallback.to_owned(), |(_, owner)| owner.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nextest_pass_and_leak_lines_give_each_test_its_slowest_time_and_a_strace_summary_keeps_the_top_syscalls_in_one_line() {
        let log = "        PASS [   0.027s] (  1/750) north::it capacity_live::fast\n        PASS [  16.402s] (  2/750) north::it slow::sleeps\n        FAIL [  30.000s] (  3/750) north::it slow::fails\n    Starting 750 tests across 18 binaries\n        PASS [  17.000s] (  4/750) north::it slow::sleeps\n        LEAK [  17.500s] (  5/750) north::it slow::leaky\n";
        let times = passed(log);
        assert_eq!(times.len(), 3);
        assert_eq!(times[&("north::it".to_owned(), "slow::sleeps".to_owned())], 17.0);
        assert_eq!(times[&("north::it".to_owned(), "slow::leaky".to_owned())], 17.5);
        let output = format!(
            "test result: ok\n{PROFILE_MARK}\n% time     seconds  usecs/call     calls    errors syscall\n------ ----------- ----------- --------- --------- ------------------\n 99.90   16.000500    16000500         1           clock_nanosleep\n  0.06    0.009600           3      3200       121 read\n  0.04    0.006400           2         7           futex\n------ ----------- ----------- --------- --------- ------------------\n100.00   16.016500                  3208       121 total\n"
        );
        let summary = summarize(&output).unwrap();
        assert!(summary.contains("16.02 s of syscall wall time in all"), "{summary}");
        assert!(summary.contains("clock_nanosleep 99.9% 16.00 s in 1 calls; read 0.1% 0.01 s in 3200 calls; futex 0.0% 0.01 s in 7 calls."), "{summary}");
        assert!(!summary.contains('\n'));
        let clean = format!("{PROFILE_MARK}\n 99.00   1.000000     1000000         1           clock_nanosleep\n100.00   1.010000                     5 total\n");
        assert!(summarize(&clean).unwrap().contains("1.01 s of syscall wall time in all"));
        assert_eq!(summarize("no table here"), None);
        assert_eq!(summarize(&format!("{PROFILE_MARK}\n")), None);
    }

    #[test]
    fn a_tests_newest_times_come_from_the_newest_land_check_logs_that_name_it_and_too_few_runs_is_no_pass() {
        use std::io::Write;
        use std::time::{Duration, SystemTime};
        let state = tempfile::tempdir().unwrap();
        let logs = state.path().join("north/land-queue/queue/logs");
        std::fs::create_dir_all(&logs).unwrap();
        let write = |name: &str, at: u64, pass: Option<f64>| {
            let path = logs.join(name);
            let mut file = std::fs::File::create(&path).unwrap();
            if let Some(seconds) = pass {
                writeln!(file, "        PASS [   {seconds:.3}s] (  1/2) north::it slow::eight").unwrap();
            }
            writeln!(file, "        PASS [   0.100s] (  2/2) north::it slow::other").unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(at)).unwrap();
        };
        write("a-candidate.log", 10, Some(9.0));
        write("b-candidate.log", 20, Some(8.0));
        write("c-candidate.log", 30, Some(7.0));
        write("d-candidate.log", 40, None);
        assert_eq!(newest(state.path(), "slow-test:north::it:slow::eight", 3), Newest::Runs(vec![9.0, 8.0, 7.0]));
        assert_eq!(newest(state.path(), "slow-test:north::it:slow::eight", 2), Newest::Runs(vec![8.0, 7.0]), "a run that skipped the test is not a run of it");
        assert_eq!(newest(state.path(), "slow-test:north::it:slow::eight", 4), Newest::Missing { found: 3, want: 4 }, "fewer runs than asked never reads as a pass");
        assert_eq!(newest(state.path(), "slow-test:north::it:slow::absent", 1), Newest::Missing { found: 0, want: 1 });
    }

}
