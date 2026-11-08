use super::finding::Finding;
use super::fix::Fix;
use super::rust_text::RustFile;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub(super) const RULE: &str = "ambient-test-input";

static HOST_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(statvfs|available_space|available_parallelism)\s*\(").unwrap());
static HOST_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""(/proc)\b"#).unwrap());
static CLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:(Instant|SystemTime)\s*::\s*now|(thread|time)\s*::\s*(?:\{[^}]*)?\bsleep)\b").unwrap());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Input {
    Host,
    Clock,
}

impl Input {
    pub(super) fn named(name: &str) -> Option<Self> {
        match name {
            "host" => Some(Self::Host),
            "clock" => Some(Self::Clock),
            _ => None,
        }
    }

    fn reads(self) -> &'static str {
        match self {
            Self::Host => "reads the real host",
            Self::Clock => "reads the wall clock or orders events by sleeping",
        }
    }

    fn found(self, file: &RustFile) -> Vec<(usize, String)> {
        match self {
            Self::Host => {
                let calls = HOST_CALL.captures_iter(file.code()).filter_map(|found| found.get(1)).map(|found| (found.start(), found.as_str().to_owned()));
                let files = HOST_FILE.captures_iter(file.commentless()).filter_map(|found| found.get(1)).map(|found| (found.start(), found.as_str().to_owned()));
                file.in_tests(calls.chain(files))
            }
            Self::Clock => file.in_tests(CLOCK.captures_iter(file.code()).filter_map(|found| {
                let (kind, verb) = found.get(1).map_or_else(|| (found.get(2), "sleep"), |kind| (Some(kind), "now"));
                kind.map(|kind| (kind.start(), format!("{}::{verb}", kind.as_str())))
            })),
        }
    }
}

pub(super) struct Ambient {
    pub(super) input: Input,
    pub(super) fixtures: Vec<String>,
    pub(super) allowlist: String,
    pub(super) fixture: String,
    pub(super) allowed: BTreeMap<String, usize>,
}

impl Ambient {
    pub(super) fn read_allowed(&mut self, text: &str) -> Result<(), String> {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let entry = line.split_once('\t').and_then(|(path, count)| Some((path.to_owned(), count.trim().parse().ok()?)));
            let (path, count) = entry.ok_or_else(|| format!("{}: expected <path><TAB><count>, got {line:?}", self.allowlist))?;
            self.allowed.insert(path, count);
        }
        Ok(())
    }

    fn reads(&self, file: &RustFile) -> Vec<(usize, String)> {
        if self.fixtures.contains(&file.path) { Vec::new() } else { self.input.found(file) }
    }

    pub(super) fn findings(&self, files: &[RustFile], tree: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
        for file in files {
            let reads = self.reads(file);
            counted.insert(&file.path, reads.len());
            let allowed = self.allowed.get(&file.path).copied().unwrap_or(0);
            if reads.len() <= allowed {
                continue;
            }
            for (line, item) in reads {
                let text = format!("{item} in test code {} — {}; {} lists {allowed} such reads for this file and only shrinks", self.input.reads(), self.fixture, self.allowlist);
                let fix = Fix::declared_for(&item);
                out.push(Finding { rule: RULE, path: file.path.clone(), line, item, text, fix });
            }
        }
        if tree {
            for (path, allowed) in &self.allowed {
                let found = counted.get(path.as_str()).copied().unwrap_or(0);
                if found < *allowed {
                    let text = format!("{} lists {allowed} such reads in tests but the file has {found} — lower the count to {found} (delete the line at 0)", self.allowlist);
                    out.push(Finding { rule: RULE, path: path.clone(), line: 0, item: String::new(), text, fix: None });
                }
            }
        }
        out
    }
}
