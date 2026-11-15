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
    pub owner: Option<String>,
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
                owner: slow.owner.clone(),
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

/// Every finished land check log the slow-test detector reads, oldest first: the lander's candidate,
/// speculative and post-land logs under `north/land-queue`, and the `safe-push` checks before queueing,
/// whose nextest output moved to `safe-push/land-check-*.log`; the box's Done when and the finding that
/// filed it read the same runs.
pub(crate) fn check_logs(state: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(state.join("north/land-queue"))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|queue| fs::read_dir(queue.path().join("logs")).into_iter().flatten().flatten())
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| check_log(name, entry.path().with_extension("outcome").is_file())))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    logs.extend(
        fs::read_dir(state.join("safe-push"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_str().is_some_and(|name| name.starts_with("land-check-") && name.ends_with(".log")))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path()))),
    );
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

pub fn owner_of(file: &str, documents: &[Document]) -> Option<String> {
    let reach = |path: &String| (file == path || file.starts_with(&format!("{}/", path.trim_end_matches('/')))).then_some(path.len());
    documents
        .iter()
        .filter_map(|document| Some((document.paths.iter().filter_map(reach).max()?, &document.owner)))
        .max_by_key(|(length, _)| *length)
        .map(|(_, owner)| owner.clone())
}
