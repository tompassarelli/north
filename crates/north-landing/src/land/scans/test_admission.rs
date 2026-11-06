use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as BytesRegex;

use crate::platform::push_ledger;
use super::test_integrity::{in_vendored, vendored_trees};
use super::incident_guard;
use super::test_file::is_test_file;
use super::{Range, git};
use crate::dep_gate::trailer_lines;
use crate::landing::fails_first;
use crate::watch::state_home;

mod kills;
mod limits;

const CONTRACT_FORMAT: &str = "Test-contract: <test name>; category: <category>";

static TEST_MARKER: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"^[+-]\s*(#\[(?:\w+::)*(?:test|test_case|proptest|rstest)\b|(?:\w+::)*proptest!\s*[({])")
        .expect("test marker pattern")
});
static SCENARIO: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"^[+-]\s*printf\s+'PASS:").expect("scenario pattern"));
static RECORD_FILE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?i)(^|/)(golden|goldens|snapshots?|__snapshots__)/|\.(snap|golden)$|[._-](golden|snapshot)s?\.[^/]*$")
        .expect("record file pattern")
});
static ADDED_FN: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"^\+\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").expect("added fn pattern")
});
static CONTRACT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\ATest-contract:\s+(\S.*?);\s*category:\s*([a-z0-9][a-z0-9-]*)\s*\z").expect("contract pattern")
});

struct Section<'a> {
    path: &'a [u8],
    created: bool,
    deleted: bool,
    header: Vec<&'a [u8]>,
    hunks: Vec<Vec<&'a [u8]>>,
}

#[derive(Default, Debug, Eq, PartialEq)]
struct Admission {
    patch: Vec<u8>,
    added: usize,
    deleted: usize,
    files: Vec<String>,
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
            parsed.push(Section { path: path_of(line), created: false, deleted: false, header: vec![line], hunks: Vec::new() });
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
            if line.starts_with(b"deleted file mode") {
                section.deleted = true;
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

fn scenarios(section: &Section, sign: u8) -> usize {
    let edited = !section.created && !section.deleted && section.path.ends_with(b".sh") && is_test_file(section.path);
    if !edited {
        return 0;
    }
    section.hunks.iter().flatten().filter(|line| line.first() == Some(&sign) && SCENARIO.is_match(line)).count()
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
    let kind = |section: &Section| is_test_file(section.path) || RECORD_FILE.is_match(section.path);
    let files = |flag: fn(&Section) -> bool| live.iter().filter(|section| flag(section) && !rust(section) && kind(section)).count();
    let (created, gone) = (files(|section| section.created), files(|section| section.deleted));
    let scenario = |sign: u8| live.iter().map(|section| scenarios(section, sign)).sum::<usize>();
    let mut found = Admission { added: added + created + scenario(b'+'), deleted: removed + gone + scenario(b'-'), ..Admission::default() };
    for section in live {
        let named = is_test_file(section.path);
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
        found.files.push(String::from_utf8_lossy(section.path).into_owned());
        found.patch.extend(lines(section.header.iter()));
        found.patch.extend(lines(kept.iter().flatten()));
    }
    found
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

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let found = changes(directory, base, head);
    if found.patch.is_empty() {
        return 0;
    }
    let listed = limits::categories(directory, &git::merge_base(directory, base, head));
    let claims = contracts(&messages(directory, base, head));
    let unlisted = limits::unlisted(&claims, &listed, &found.patch);
    let contracted = claims.len() - unlisted.len();
    let mut refusals: Vec<String> = unlisted
        .into_iter()
        .map(|reason| format!("safe-push: ✗ test admission: {reason}; a contract test names a category from test-admission.contract in .safe-push on main with the trailer {CONTRACT_FORMAT}, or guards a bug instead; NOT pushed.\n"))
        .collect();
    refusals.extend(over_budget(directory, base, head, &found, contracted, out));
    for refusal in &refusals {
        let _ = out.write_all(refusal.as_bytes());
    }
    u8::from(!refusals.is_empty())
}

fn contracts(messages: &[String]) -> Vec<limits::Claim> {
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| CONTRACT.captures(line))
        .map(|found| limits::Claim { test: found[1].trim().to_owned(), category: found[2].to_owned() })
        .collect()
}

pub fn contract_tests(directory: &Path, rev: &str) -> Vec<String> {
    let listed = limits::categories(directory, rev);
    let history: Vec<String> = git::text(directory, &["log", "--grep=Test-contract:", "-i", "--format=%B%x00", rev])
        .unwrap_or_default()
        .split('\0')
        .map(str::to_owned)
        .collect();
    contracts(&history).into_iter().filter(|claim| listed.contains(&claim.category.to_ascii_lowercase())).map(|claim| claim.test).collect()
}

fn over_budget(directory: &Path, base: &str, head: &str, found: &Admission, contracted: usize, out: &mut impl Write) -> Option<String> {
    let added = found.added.saturating_sub(contracted);
    let origin = git::text(directory, &["remote", "get-url", "origin"]).unwrap_or_default();
    let repository = push_ledger::repository(origin.trim());
    let measured = limits::measured_seconds(&state_home(), &repository)?;
    let budget = limits::budget_seconds();
    if measured <= budget || found.deleted >= added {
        return None;
    }
    let bugs = incident_guard::fixes(directory, &git::merge_base(directory, base, head), head).bugs;
    let files = found.files.join(", ");
    if !bugs.is_empty() && found.deleted + bugs.len() >= added {
        let _ = writeln!(out, "safe-push: test admission: counts the test{} in {files} as the regression test of bug {}, not as growth of the test budget", plural(added), incident_guard::named(&bugs));
        return None;
    }
    let incident = if bugs.is_empty() { "a bug-labelled north issue its commit subject cites".to_owned() } else { incident_guard::named(&bugs) };
    Some(format!(
        "safe-push: ✗ test admission: {repository}'s verification takes {measured} s (median green post-land run), over its {budget} s budget, and the lane adds {added} test{} in {files} but deletes {}; over budget a test lands only with as many deleted (one in, one out), unless it pins a listed contract or is the one regression test of each bug the lane fixes ({incident}); NOT pushed.\n",
        plural(added),
        found.deleted
    ))
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
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

pub use kills::run as kills;

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
