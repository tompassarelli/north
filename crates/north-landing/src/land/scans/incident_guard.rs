use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::test_file::is_test_file;
use super::{Range, git};
use crate::platform::land_queue::{ROUND, slug};
use crate::watch::facts::slug_of;
use crate::watch::state_home;

const NORTH: &str = "tompassarelli/north";
const BUG: &str = "bug";
const FRESH: Duration = Duration::from_secs(3600);

static NORTH_CITATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bnorth#([0-9]+)").expect("citation pattern"));
static FIXES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bfix(?:es|ed)?\s+#([0-9]+)").expect("fixes pattern"));
static TEST_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\+\s*#\[(?:[[:alnum:]_]+::)*test\b").expect("test attribute pattern"));

fn numbers<'a>(pattern: &'a Regex, text: &'a str) -> impl Iterator<Item = u64> + 'a {
    pattern.captures_iter(text).filter_map(|found| found[1].parse().ok())
}

fn cited(subjects: &str, origin_is_north: bool) -> BTreeSet<u64> {
    let bare = origin_is_north.then(|| numbers(&FIXES, subjects)).into_iter().flatten();
    numbers(&NORTH_CITATION, subjects).chain(bare).collect()
}

fn needs_no_test(directory: &Path, base: &str, head: &str) -> bool {
    let names = git::text(directory, &["diff", "--name-only", "--no-renames", base, head]).unwrap_or_default();
    if names.lines().all(|path| path.ends_with(".md")) || names.lines().any(is_test_file) {
        return true;
    }
    let diff = git::text(directory, &["diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-renames", base, head]).unwrap_or_default();
    TEST_ATTRIBUTE.is_match(&diff)
}

struct Labels {
    cache: Option<PathBuf>,
}

impl Labels {
    fn for_round() -> Self {
        let round = std::env::var(ROUND).unwrap_or_default();
        let cache = (!round.is_empty()).then(|| {
            let directory = state_home().join("north/land/issue-labels");
            prune(&directory);
            directory.join(slug(&round))
        });
        Self { cache }
    }

    fn of(&self, number: u64) -> Option<String> {
        let cached = self.cache.as_ref().map(|directory| directory.join(format!("north-{number}")));
        if let Some(text) = cached.as_deref().filter(|path| fresh(path)).and_then(|path| fs::read_to_string(path).ok()) {
            return Some(text);
        }
        let output = Command::new("timeout").args(["10", "gh", "api", &format!("repos/{NORTH}/issues/{number}"), "--jq", ".labels[].name"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if let Some(path) = cached
            && path.parent().is_some_and(|parent| fs::create_dir_all(parent).is_ok())
        {
            let _ = fs::write(path, &text);
        }
        Some(text)
    }
}

fn fresh(path: &Path) -> bool {
    fs::metadata(path).and_then(|meta| meta.modified()).is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age < FRESH))
}

fn prune(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !fresh(&path) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

pub(crate) struct Fixes {
    pub bugs: Vec<u64>,
    pub unread: Vec<u64>,
}

pub(crate) fn fixes(directory: &Path, base: &str, head: &str) -> Fixes {
    let subjects = git::text(directory, &["log", "--no-merges", "--format=%s", &format!("{base}..{head}")]).unwrap_or_default();
    let origin = git::text(directory, &["remote", "get-url", "origin"]).and_then(|url| slug_of(&url));
    let labels = Labels::for_round();
    let mut found = Fixes { bugs: Vec::new(), unread: Vec::new() };
    for number in cited(&subjects, origin.as_deref() == Some(NORTH)) {
        match labels.of(number) {
            Some(text) if text.lines().any(|label| label.trim() == BUG) => found.bugs.push(number),
            Some(_) => {}
            None => found.unread.push(number),
        }
    }
    found
}

pub(crate) fn named(issues: &[u64]) -> String {
    issues.iter().map(|number| format!("north#{number}")).collect::<Vec<_>>().join(", ")
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let base = git::merge_base(directory, base, head);
    if needs_no_test(directory, &base, head) {
        return 0;
    }
    let Fixes { bugs, unread } = fixes(directory, &base, head);
    if !unread.is_empty() {
        let _ = writeln!(out, "safe-push: ⚠ GitHub did not answer for the labels of {}, so the incident-test scan passes it unchecked", named(&unread));
    }
    if bugs.is_empty() {
        return 0;
    }
    let _ = writeln!(out, "safe-push: ✗ this lane fixes bug {} but adds or changes no test; add a test that fails without the fix (a file under tests/ or test/, one named tests.*, *.test.*, *_test.* or *_tests.*, or a #[test]); NOT pushed.", named(&bugs));
    1
}
