use super::habits::Finding;
use super::rust_text::RustFile;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub(super) const RULE: &str = "ambient-test-input";

static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(thread\s*::\s*sleep|time\s*::\s*sleep(?:_until)?|Instant\s*::\s*now|SystemTime\s*::\s*now)\s*\(").unwrap()
});
static PAUSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bstart_paused\b|\btime\s*::\s*pause\s*\(").unwrap());
static HOST_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(statvfs|available_space|available_parallelism)\s*\(").unwrap());
static HOST_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""(/proc)\b"#).unwrap());

pub(super) struct Ambient {
    pub(super) fixtures: Vec<String>,
    pub(super) allowlist: String,
    pub(super) time_fixture: String,
    pub(super) host_fixture: String,
    pub(super) refuse: bool,
    pub(super) allowed: BTreeMap<String, usize>,
}

impl Ambient {
    pub(super) fn read_allowed(&mut self, text: &str) -> Result<(), String> {
        for line in text.lines().filter(|line| !line.trim().is_empty() && !line.starts_with('#')) {
            let entry = line.split_once('\t').and_then(|(path, count)| Some((path.to_owned(), count.trim().parse().ok()?)));
            let (path, count) = entry.ok_or_else(|| format!("{}: expected <path><TAB><count>, got {line:?}", self.allowlist))?;
            self.allowed.insert(path, count);
        }
        Ok(())
    }

    fn exempt(&self, file: &RustFile) -> bool {
        let examples = file.path.split('/').any(|part| matches!(part, "examples" | "benches"));
        examples || self.fixtures.contains(&file.path)
    }

    fn sites(&self, file: &RustFile) -> Vec<(usize, String, &str)> {
        if self.exempt(file) {
            return Vec::new();
        }
        let paused = PAUSED.is_match(file.masked());
        let clock = CLOCK.captures_iter(file.masked()).filter_map(|found| found.get(1)).filter_map(|call| {
            let name: String = call.as_str().split_whitespace().collect();
            (!(paused && name.starts_with("time::sleep"))).then_some((call.start(), name, self.time_fixture.as_str()))
        });
        let calls = HOST_CALL.captures_iter(file.masked()).filter_map(|found| found.get(1));
        let reads = HOST_FILE.captures_iter(file.commentless()).filter_map(|found| found.get(1));
        let host = calls.chain(reads).map(|found| (found.start(), found.as_str().to_owned(), self.host_fixture.as_str()));
        let mut sites: Vec<_> = clock.chain(host).filter(|(at, _, _)| file.is_test(*at)).map(|(at, name, fixture)| (file.line(at), name, fixture)).collect();
        sites.sort();
        sites
    }

    pub(super) fn findings(&self, files: &[RustFile], tree: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
        for file in files {
            let sites = self.sites(file);
            counted.insert(&file.path, sites.len());
            let allowed = self.allowed.get(&file.path).copied().unwrap_or(0);
            if sites.len() <= allowed {
                continue;
            }
            for (line, name, fixture) in sites {
                let text = format!("{name} in test code reads {} — {fixture}; {} lists {allowed} such uses for this file and only shrinks", if fixture == self.time_fixture { "the wall clock" } else { "the real host" }, self.allowlist);
                out.push(Finding { rule: RULE, path: file.path.clone(), line, item: name, text });
            }
        }
        if tree {
            for (path, allowed) in &self.allowed {
                let found = counted.get(path.as_str()).copied().unwrap_or(0);
                if found < *allowed {
                    let text = format!("{} lists {allowed} ambient test inputs but the file has {found} — lower the count to {found} (delete the line at 0)", self.allowlist);
                    out.push(Finding { rule: RULE, path: path.clone(), line: 0, item: String::new(), text });
                }
            }
        }
        out
    }
}
