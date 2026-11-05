use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use serde_json::Value;

use super::{Range, git, host_paths};

static MUTANT_MARKER: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"MUTANT:[[:alnum:]_-]").expect("mutant pattern"));
static TEST_PATH: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(^|/)tests?/|[._]tests?\.[^/]*$|(^|/)test_[^/]*\.py$")
        .expect("test path pattern")
});
static SKIP_MARKER: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(^|[^[:alnum:]_])(it|test|describe)\.(skip|only)\(|#\[ignore|pytest\.mark\.(skip|xfail)|unittest\.skip")
        .expect("skip pattern")
});
static TREE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[[:alnum:]_-][[:alnum:]._-]*(/[[:alnum:]_-][[:alnum:]._-]*)*$")
        .expect("tree pattern")
});
static TEST_CHANGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^Test-change:[[:space:]]*[^[:space:]]").expect("trailer pattern")
});

fn declared(directory: &Path, base: &str, path: &str, key: &str) -> Option<String> {
    let json = git::bytes(directory, &["show", &format!("{base}:{path}")])?;
    match serde_json::from_slice::<Value>(&json).ok()?.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(true) => Some("true".to_owned()),
        _ => None,
    }
}

pub(super) fn vendored_trees(directory: &Path, base: &str) -> Vec<String> {
    let listing =
        git::text(directory, &["ls-tree", "--name-only", base, "repos/"]).unwrap_or_default();
    let repos = listing
        .lines()
        .filter(|entry| entry.starts_with("repos/") && entry.ends_with(".json"))
        .filter_map(|entry| declared(directory, base, entry, "prefix"));
    declared(directory, base, "effect-kit.json", "vendor")
        .into_iter()
        .chain(repos)
        .flat_map(|value| value.lines().map(str::to_owned).collect::<Vec<_>>())
        .filter(|tree| TREE_PATH.is_match(tree))
        .collect()
}

pub(super) fn in_vendored(path: &[u8], trees: &[String]) -> bool {
    trees.iter().map(String::as_bytes).any(|tree| {
        path.strip_prefix(tree)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
    })
}

fn refusal(out: &mut impl Write, path: &[u8], what: &str, line: &[u8], remedy: &str) {
    let mut text = Vec::new();
    text.extend_from_slice("safe-push: ✗ ".as_bytes());
    text.extend_from_slice(path);
    text.extend_from_slice(format!(" adds {what}: ").as_bytes());
    text.extend_from_slice(line);
    text.extend_from_slice(format!(" — {remedy}; NOT pushed.\n").as_bytes());
    let _ = out.write_all(&text);
}

fn judge(diff: &[u8], trees: &[String], skip_allowed: bool, out: &mut impl Write) -> bool {
    let (mut header, mut vendored, mut bad) = (false, false, false);
    let mut path: &[u8] = b"";
    for line in diff.split(|byte| *byte == b'\n') {
        if line.starts_with(b"diff --git ") {
            header = true;
            continue;
        }
        if header && line.starts_with(b"+++ ") {
            path = line.get(6..).unwrap_or_default();
            header = false;
            vendored = in_vendored(path, trees);
            continue;
        }
        if header || vendored {
            continue;
        }
        let Some(added) = line.strip_prefix(b"+") else {
            continue;
        };
        if MUTANT_MARKER.is_match(line) {
            refusal(
                out,
                path,
                "a mutation marker",
                added,
                "mutants stay in scratch copies",
            );
            bad = true;
        } else if !skip_allowed && TEST_PATH.is_match(path) && SKIP_MARKER.is_match(line) {
            refusal(
                out,
                path,
                "a skip or focus",
                added,
                "name the changed requirement in a Test-change: trailer",
            );
            bad = true;
        } else if let Some(found) = host_paths::find(path, added) {
            refusal(
                out,
                path,
                &format!("the host path {}", String::from_utf8_lossy(found)),
                added,
                "tests run on hosts without it; build a PATH with support::test_env or use a path every host has",
            );
            bad = true;
        }
    }
    bad
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let base = git::merge_base(directory, base, head);
    let trees = vendored_trees(directory, &base);
    let skip_allowed = TEST_CHANGE.is_match(&git::messages(directory, &base, head));
    let diff = git::bytes(
        directory,
        &[
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            &base,
            head,
        ],
    )
    .unwrap_or_default();
    u8::from(judge(&diff, &trees, skip_allowed, out))
}
