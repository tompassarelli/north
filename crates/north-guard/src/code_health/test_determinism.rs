use super::habits::Finding;
use super::rust_text::RustFile;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

pub(super) const RULE: &str = "test-nondeterminism";
pub(super) const ALLOWLIST: &str = ".test-determinism-allowlist.json";

const CLOCK: &str = "wait on a persisted event, a channel or a drained worker instead of a sleep or a poll; run timeouts under #[tokio::test(start_paused = true)] with tokio::time::advance, and pin an instant with Timestamp::from_unix_seconds in place of the wall clock";
const HOST: &str = "point HOME and XDG_STATE_HOME at a tempdir and pin the readings the code takes (AGENT_CAPACITY_DISK_FREE_GIB, AGENT_CAPACITY_CORES, AGENT_CAPACITY_LOADAVG, AGENT_CAPACITY_MEMINFO) in a fixtures module; an existing use stays in the allowlist, which only shrinks";

static STD_SLEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bthread\s*::\s*sleep\s*\(").unwrap());
static TOKIO_SLEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\btime\s*::\s*sleep\s*\(").unwrap());
static INSTANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bInstant\s*::\s*now\s*\(").unwrap());
static SYSTEM_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bSystemTime\s*::\s*now\s*\(").unwrap());
static HOST_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bstatvfs\s*\(|\bhome_dir\s*\(").unwrap());
static HOST_TEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""/proc/|\.local/state|var(?:_os)?\(\s*"HOME"\s*\)"#).unwrap());

type Counts = BTreeMap<String, usize>;

pub(super) fn exempt(path: &str) -> bool {
    let mut parts = path.rsplit('/');
    let file = parts.next().unwrap_or(path);
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    matches!(stem, "fixtures" | "test_support" | "harness") || stem.ends_with("_fixture") || stem.ends_with("_fixtures") || parts.any(|part| matches!(part, "fixtures" | "support"))
}

fn sites(file: &RustFile) -> Vec<(usize, String, &'static str)> {
    let paused = file.mentions("start_paused") || file.mentions("time::pause");
    let mut found = Vec::new();
    let mut take = |matches: Vec<(usize, String)>, remedy| found.extend(matches.into_iter().map(|(line, text)| (line, text, remedy)));
    take(file.test_matches(&STD_SLEEP, false), CLOCK);
    take(file.test_matches(&SYSTEM_TIME, false), CLOCK);
    if !paused {
        take(file.test_matches(&TOKIO_SLEEP, false), CLOCK);
        take(file.test_matches(&INSTANT, false), CLOCK);
    }
    take(file.test_matches(&HOST_CODE, false), HOST);
    take(file.test_matches(&HOST_TEXT, true), HOST);
    found.sort_by_key(|(line, ..)| *line);
    found
}

pub(super) fn load(root: &Path) -> Result<Counts, String> {
    match std::fs::read(root.join(ALLOWLIST)) {
        Ok(bytes) => parse(&bytes),
        Err(_) => Ok(Counts::new()),
    }
}

fn parse(bytes: &[u8]) -> Result<Counts, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("{ALLOWLIST}: {error}"))
}

pub(super) fn check(files: &[RustFile], allowed: &Counts, whole_tree: bool) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = BTreeMap::new();
    for file in files.iter().filter(|file| !exempt(&file.path)) {
        let found = sites(file);
        let listed = allowed.get(&file.path).copied().unwrap_or(0);
        seen.insert(file.path.clone(), found.len());
        if found.len() > listed {
            out.extend(found.into_iter().map(|(line, text, remedy)| Finding {
                rule: RULE,
                path: file.path.clone(),
                line,
                text: format!("{} in test code — {remedy}; {ALLOWLIST} allows {listed} here", text.trim_end_matches('(').trim()),
                item: text,
                advisory: false,
            }));
        }
    }
    if whole_tree {
        for (path, listed) in allowed {
            let now = seen.get(path).copied().unwrap_or(0);
            if now < *listed {
                let text = format!("{ALLOWLIST} lists {listed} for {path} but it has {now} now — lower the entry, and remove it at 0");
                out.push(Finding { rule: RULE, path: path.clone(), line: 0, text, item: String::new(), advisory: false });
            }
        }
    }
    out
}

pub(super) fn grown(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let shown = Command::new("git").arg("-C").arg(root).args(["show", &format!("{base}:{ALLOWLIST}")]).output().map_err(|error| format!("git show: {error}"))?;
    if !shown.status.success() {
        return Ok(Vec::new());
    }
    let before = parse(&shown.stdout)?;
    Ok(load(root)?
        .into_iter()
        .filter(|(path, now)| *now > before.get(path).copied().unwrap_or(0))
        .map(|(path, now)| format!("{ALLOWLIST}: {path} lists {now}, up from {}; the allowlist only shrinks — {CLOCK}", before.get(&path).copied().unwrap_or(0)))
        .collect())
}
