use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as BytesRegex;

use super::test_integrity::{in_vendored, vendored_trees};
use super::{Range, git};
use crate::dep_gate::trailer_lines;
use crate::landing::fails_first;

const TRAILER_FORMAT: &str =
    "Test-review: keep N of M; reviewer: <session>; patch-id <id from north land scan test-admission id BASE..HEAD>";

static TEST_MARKER: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"^[+-]\s*(#\[(?:\w+::)*(?:test|test_case|proptest|rstest)\b|(?:\w+::)*proptest!\s*[({])")
        .expect("test marker pattern")
});
static TEST_FILE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(^|/)tests?/|[._]tests?\.[^/]*$").expect("test file pattern")
});
static RECORD_FILE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?i)(^|/)(golden|goldens|snapshots?|__snapshots__)/|\.(snap|golden)$|[._-](golden|snapshot)s?\.[^/]*$")
        .expect("record file pattern")
});
static ADDED_FN: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"^\+\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").expect("added fn pattern")
});
static REVIEW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\ATest-review:\s+keep\s+(\d+)\s+of\s+(\d+);\s*reviewer:\s*([a-z0-9](?:[a-z0-9._:/@_-]*[a-z0-9])?);\s*patch-id\s+([0-9a-f]{40,64})\s*\z")
        .expect("review pattern")
});

struct Section<'a> {
    path: &'a [u8],
    created: bool,
    header: Vec<&'a [u8]>,
    hunks: Vec<Vec<&'a [u8]>>,
}

#[derive(Default, Debug, Eq, PartialEq)]
struct Admission {
    patch: Vec<u8>,
    tests: usize,
    files: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct Review {
    kept: usize,
    total: usize,
    reviewer: String,
    id: String,
}

#[derive(Debug, Eq, PartialEq)]
enum Verdict {
    Admitted,
    Missing,
    Stale { reviewed: Vec<String> },
    Malformed { kept: usize, total: usize, expected: usize },
    SelfReviewed { reviewer: String },
}

fn path_of(line: &[u8]) -> &[u8] {
    let rest = line.strip_prefix(b"diff --git ").unwrap_or_default();
    let Some(length) = rest.len().checked_sub(5).filter(|extra| extra % 2 == 0).map(|extra| extra / 2) else {
        return b"";
    };
    let (old, new) = (rest.get(2..2 + length), rest.get(5 + length..));
    match (old, new) {
        (Some(old), Some(new)) if old == new && rest.starts_with(b"a/") && rest[2 + length..].starts_with(b" b/") => old,
        _ => b"",
    }
}

fn sections(diff: &[u8]) -> Vec<Section<'_>> {
    let mut parsed: Vec<Section> = Vec::new();
    for line in diff.split(|byte| *byte == b'\n') {
        if line.starts_with(b"diff --git ") {
            parsed.push(Section { path: path_of(line), created: false, header: vec![line], hunks: Vec::new() });
            continue;
        }
        let Some(section) = parsed.last_mut() else { continue };
        if line.starts_with(b"@@") {
            section.hunks.push(vec![line]);
        } else if let Some(hunk) = section.hunks.last_mut() {
            hunk.push(line);
        } else {
            if line.starts_with(b"new file mode") {
                section.created = true;
            }
            if let Some(path) = line.strip_prefix(b"rename to ") {
                section.path = path;
            }
            section.header.push(line);
        }
    }
    parsed
}

fn lines(parts: impl Iterator<Item = impl AsRef<[u8]>>) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(part.as_ref());
        out.push(b'\n');
    }
    out
}

fn marked(hunk: &[&[u8]], sign: u8) -> usize {
    hunk.iter().filter(|line| line.first() == Some(&sign) && TEST_MARKER.is_match(line)).count()
}

fn count(section: &Section, sign: u8) -> usize {
    section.hunks.iter().map(|hunk| marked(hunk, sign)).sum()
}

static HUNK_HEADER: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@").expect("hunk header pattern"));

fn brace_change(text: &[u8]) -> (i32, bool) {
    let (mut net, mut seen, mut quoted, mut index) = (0, false, false, 0);
    while let Some(&byte) = text.get(index) {
        index += 1;
        match byte {
            b'\\' if quoted => index += 1,
            b'"' => quoted = !quoted,
            b'\'' if !quoted && text.get(index + 1) == Some(&b'\'') => index += 2,
            b'{' if !quoted => (net, seen) = (net + 1, true),
            b'}' if !quoted => net -= 1,
            _ => {}
        }
    }
    (net, seen)
}

fn tests_only(hunk: &[&[u8]]) -> Vec<Vec<u8>> {
    let owned = || hunk.iter().map(|line| line.to_vec()).collect();
    let starts = |line: &&[u8]| line.first() == Some(&b'+') && (TEST_MARKER.is_match(line) || line[1..].trim_ascii().starts_with(b"#[cfg(test)]"));
    let Some(first) = hunk.iter().position(starts) else { return owned() };
    let Some(header) = hunk.first().and_then(|line| HUNK_HEADER.captures(line)) else { return owned() };
    let number = |bytes: &[u8]| String::from_utf8_lossy(bytes).parse::<usize>().unwrap_or(0);
    let (start, old) = (number(&header[1]), header.get(2).map_or(1, |count| number(count.as_bytes())));
    let after = if old == 0 { start } else { start + old - 1 };
    let (mut kept, mut depth, mut opened, mut skipping) = (Vec::new(), 0_i32, false, false);
    for line in &hunk[first..] {
        match line.first() {
            Some(b'\\') => kept.push(line.to_vec()),
            Some(b'+') => {
                let text = line[1..].trim_ascii();
                let begins = starts(line);
                if skipping && !begins {
                    continue;
                }
                if opened && depth <= 0 && !begins && !text.starts_with(b"#[") {
                    skipping = true;
                    continue;
                }
                if skipping || opened && depth <= 0 {
                    (skipping, opened, depth) = (false, false, 0);
                }
                kept.push(line.to_vec());
                let (net, seen) = brace_change(text);
                (depth, opened) = (depth + net, opened || seen);
            }
            _ => {}
        }
    }
    let count = kept.iter().filter(|line| line.first() == Some(&b'+')).count();
    let mut trimmed = vec![format!("@@ -{after},0 +{},{count} @@", after + 1).into_bytes()];
    trimmed.extend(kept);
    trimmed
}

fn admission(diff: &[u8], trees: &[String]) -> Admission {
    let all = sections(diff);
    let live: Vec<&Section> = all.iter().filter(|section| !section.path.is_empty() && !in_vendored(section.path, trees)).collect();
    let rust = |section: &Section| section.path.ends_with(b".rs");
    let added: usize = live.iter().filter(|section| rust(section)).map(|section| count(section, b'+')).sum();
    let removed: usize = live.iter().filter(|section| rust(section)).map(|section| count(section, b'-')).sum();
    let net = added.saturating_sub(removed);
    let mut found = Admission { tests: net, ..Admission::default() };
    for section in live {
        let named = TEST_FILE.is_match(section.path);
        let whole = section.created && (named || RECORD_FILE.is_match(section.path)) && (!rust(section) || net > 0);
        let kept: Vec<Vec<Vec<u8>>> = section
            .hunks
            .iter()
            .filter(|hunk| whole || (rust(section) && net > 0 && count(section, b'+') > 0 && (named || marked(hunk, b'+') > 0)))
            .map(|hunk| if whole { hunk.iter().map(|line| line.to_vec()).collect() } else { tests_only(hunk) })
            .collect();
        if kept.is_empty() && !whole {
            continue;
        }
        found.tests += usize::from(whole && !rust(section));
        found.files.push(String::from_utf8_lossy(section.path).into_owned());
        found.patch.extend(lines(section.header.iter()));
        found.patch.extend(lines(kept.iter().flatten()));
    }
    found.tests = found.tests.max(usize::from(!found.patch.is_empty()));
    found
}

fn patch_id(directory: &Path, patch: &[u8]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["patch-id", "--verbatim"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("git patch-id: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("git patch-id has no stdin")?;
    let patch = patch.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&patch));
    let output = child.wait_with_output().map_err(|error| format!("git patch-id: {error}"))?;
    writer.join().map_err(|_| "git patch-id writer panicked")?.map_err(|error| format!("git patch-id: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "git patch-id printed no id".to_owned())
}

fn reviews(messages: &[String]) -> Vec<Review> {
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| REVIEW.captures(line))
        .filter_map(|found| {
            Some(Review {
                kept: found[1].parse().ok()?,
                total: found[2].parse().ok()?,
                reviewer: found[3].to_owned(),
                id: found[4].to_ascii_lowercase(),
            })
        })
        .collect()
}

fn verdict(id: &str, reviews: &[Review], author: Option<&str>, expected: usize) -> Verdict {
    let matching: Vec<&Review> = reviews.iter().filter(|review| review.id == id).collect();
    if reviews.is_empty() {
        return Verdict::Missing;
    }
    if matching.is_empty() {
        return Verdict::Stale { reviewed: reviews.iter().map(|review| review.id.clone()).collect() };
    }
    let own = |review: &&Review| author.is_some_and(|session| session.eq_ignore_ascii_case(&review.reviewer));
    let sound = |review: &&Review| (1..=review.total).contains(&review.kept) && review.total == expected;
    match matching.iter().find(|review| sound(review) && !own(review)) {
        Some(_) => Verdict::Admitted,
        None => match matching.iter().find(|review| own(review)) {
            Some(review) => Verdict::SelfReviewed { reviewer: review.reviewer.clone() },
            None => Verdict::Malformed { kept: matching[0].kept, total: matching[0].total, expected },
        },
    }
}

fn explain(verdict: &Verdict, id: &str, range: &str, found: &Admission) -> Option<String> {
    let what = format!("adds {} test{} in {}", found.tests, if found.tests == 1 { "" } else { "s" }, found.files.join(", "));
    let rerun = format!("a fresh reviewer of a different tier argues for deleting each added test, then writes the verdict per test in the commit body and the trailer {TRAILER_FORMAT} (this range: patch-id {id}, from {range})");
    let reason = match verdict {
        Verdict::Admitted => return None,
        Verdict::Missing => format!("{what} but carries no Test-review: trailer"),
        Verdict::Stale { reviewed } => format!("{what}, and the review never saw the final test hunks: Test-review: patch-id {} does not match {id}", reviewed.join(", ")),
        Verdict::Malformed { kept, total, expected } => format!("{what}, and its review keeps {kept} of {total}, which must keep at least 1 of exactly {expected}"),
        Verdict::SelfReviewed { reviewer } => format!("{what}, and reviewer {reviewer} is the session that landed it"),
    };
    Some(format!("safe-push: ✗ test admission: the lane {reason} — {rerun}; NOT pushed.\n"))
}

fn changes(directory: &Path, base: &str, head: &str) -> Admission {
    let base = git::merge_base(directory, base, head);
    let trees = vendored_trees(directory, &base);
    let diff = git::bytes(
        directory,
        &["--literal-pathspecs", "diff", "--unified=0", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "--src-prefix=a/", "--dst-prefix=b/", "--diff-algorithm=myers", &base, head],
    )
    .unwrap_or_default();
    admission(&diff, &trees)
}

fn messages(directory: &Path, base: &str, head: &str) -> Vec<String> {
    git::text(directory, &["log", "--format=%B%x00", &format!("{base}..{head}")])
        .unwrap_or_default()
        .split('\0')
        .map(str::to_owned)
        .collect()
}

pub fn scan(directory: &Path, range: &str, author: Option<&str>, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let found = changes(directory, base, head);
    if found.patch.is_empty() {
        return 0;
    }
    let id = match patch_id(directory, &found.patch) {
        Ok(id) => id,
        Err(error) => {
            let _ = writeln!(out, "safe-push: ✗ test admission: {error}; NOT pushed.");
            return 1;
        }
    };
    let reviews = reviews(&messages(directory, base, head));
    match explain(&verdict(&id, &reviews, author, found.tests), &id, range, &found) {
        Some(text) => {
            let _ = out.write_all(text.as_bytes());
            1
        }
        None => 0,
    }
}

pub fn id(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        eprintln!("usage: north land scan test-admission id BASE..HEAD");
        return 1;
    };
    let found = changes(directory, base, head);
    if found.patch.is_empty() {
        eprintln!("test-admission: {range} adds no test");
        return 0;
    }
    match patch_id(directory, &found.patch) {
        Ok(id) => {
            let _ = writeln!(out, "patch-id {id}\ntests {}", found.tests);
            0
        }
        Err(error) => {
            eprintln!("test-admission: {error}");
            1
        }
    }
}

fn added_names(patch: &[u8]) -> BTreeSet<String> {
    let (mut names, mut armed, mut block) = (BTreeSet::new(), false, false);
    for line in patch.split(|byte| *byte == b'\n') {
        if line.starts_with(b"@@") || line.starts_with(b"diff --git ") {
            (armed, block) = (false, false);
        } else if TEST_MARKER.is_match(line) && line.first() == Some(&b'+') {
            armed = true;
            block = line.windows(9).any(|window| window == b"proptest!");
        } else if let Some(found) = ADDED_FN.captures(line).filter(|_| armed) {
            names.insert(String::from_utf8_lossy(&found[1]).into_owned());
            armed = block;
        }
    }
    names
}

fn fresh_names(directory: &Path, base: &str, patch: &[u8]) -> BTreeSet<String> {
    added_names(patch)
        .into_iter()
        .filter(|name| git::bytes(directory, &["grep", "-q", "-E", &format!("fn {name}\\b"), base, "--", "*.rs"]).is_none())
        .collect()
}

fn claims_a_bug(directory: &Path, base: &str, head: &str, patch: &[u8]) -> bool {
    let parsed = sections(patch);
    let tested = |path: &str| -> usize {
        parsed.iter().filter(|section| section.path == path.as_bytes()).flat_map(|section| section.hunks.iter().flatten()).filter(|line| matches!(line.first(), Some(b'+' | b'-'))).count()
    };
    let numbers = git::text(directory, &["diff", "--numstat", "--no-renames", base, head]).unwrap_or_default();
    let code = numbers.lines().any(|row| {
        let mut columns = row.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (columns.next(), columns.next(), columns.next()) else { return false };
        let changed = added.parse::<usize>().unwrap_or(1) + removed.parse::<usize>().unwrap_or(1);
        fails_first::is_code(path) && changed > tested(path)
    });
    code && !git::messages(directory, base, head).lines().any(|line| line.trim() == fails_first::MARK)
}

pub fn fail_to_pass(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        eprintln!("usage: north land scan test-admission run BASE..HEAD");
        return 64;
    };
    let base = git::merge_base(directory, base, head);
    let found = changes(directory, &base, head);
    let names = fresh_names(directory, &base, &found.patch);
    if names.is_empty() || !claims_a_bug(directory, &base, head, &found.patch) {
        let _ = writeln!(out, "fail-to-pass: not applicable ({range} adds no Rust test next to a code change, or is marked {})", fails_first::MARK);
        return 0;
    }
    let passing = match fails_first::pass_on_base(directory, &base, &found.patch, &names, out) {
        Ok(verdicts) => verdicts,
        Err(reason) => {
            let _ = writeln!(out, "fail-to-pass: could not run, not the lane's fault: {reason}");
            return 75;
        }
    };
    let guards: Vec<&str> = passing.iter().filter(|(_, passes)| !**passes).map(|(name, _)| name.as_str()).collect();
    let idle: Vec<&str> = passing.iter().filter(|(_, passes)| **passes).map(|(name, _)| name.as_str()).collect();
    if idle.is_empty() {
        let _ = writeln!(out, "fail-to-pass: ✓ all {} added test(s) fail on base with the lane's code changes reverted: {}", guards.len(), guards.join(", "));
        return 0;
    }
    let _ = writeln!(out, "after-landing: ✗ fail-to-pass: these added tests pass with the lane's non-test changes reverted, so none guards the change: {}; make each fail without the change, or put a line `{}` in the commit message of a behavior-preserving change.", idle.join(", "), fails_first::MARK);
    1
}
