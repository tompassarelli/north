use std::collections::BTreeSet;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use super::{bars, fields, gating, git, origin, policy_line, records, rule_items, rule_text};

pub const RECORDS: &str = "harness/provenance.md";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Kind {
    Hook,
    Policy,
    Tier,
    Bar,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Hook => "hook",
            Kind::Policy => "policy line",
            Kind::Tier => "tier",
            Kind::Bar => "gating bar",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Item {
    pub kind: Kind,
    pub path: String,
    pub key: String,
    needle: String,
}

fn key(text: &str) -> String {
    let text = rule_text(text).replace(';', ",");
    re!(r"\[([^\]]*)\]\(([^)\s]*)\)").replace_all(&text, "$1 at $2").into_owned()
}

fn hook(path: &str) -> bool {
    path.starts_with("harness/hooks/")
        && !["/lib/", "/fixtures/", ".test."].iter().any(|s| path.contains(s))
}

fn tier(path: &str) -> bool {
    path.starts_with("harness/claude/agents/") && path.ends_with(".md")
}

fn prose(path: &str) -> bool {
    path.ends_with(".md")
        && path != RECORDS
        && !["fixtures/", "tests/", "testdata/"].iter().any(|s| path.contains(s))
}

fn agents(path: &str) -> bool {
    Path::new(path).file_name().is_some_and(|n| n == "AGENTS.md")
}

pub fn items(repo: &Path, rev: &str) -> Result<Vec<Item>, String> {
    let mut found = BTreeSet::new();
    let tree = git(repo, &["ls-tree", "-r", "-z", "--name-only", rev])?;
    for path in tree.split_terminator('\0') {
        if hook(path) || tier(path) {
            found.insert(Item {
                kind: if hook(path) { Kind::Hook } else { Kind::Tier },
                path: path.into(),
                key: path.into(),
                needle: String::new(),
            });
        }
        if !agents(path) && !prose(path) {
            continue;
        }
        let text = git(repo, &["show", &format!("{rev}:{path}")])?;
        if agents(path) {
            let lines: Vec<_> = text.lines().collect();
            for (start, _, item) in rule_items(path, &text) {
                if policy_line(path, &item) {
                    found.insert(Item {
                        kind: Kind::Policy,
                        path: path.into(),
                        key: key(&item),
                        needle: lines[start - 1].trim().into(),
                    });
                }
            }
        }
        if prose(path) {
            for line in text.lines() {
                if gating(path, line) && !bars(line).is_empty() {
                    found.insert(Item {
                        kind: Kind::Bar,
                        path: path.into(),
                        key: key(line),
                        needle: line.trim().into(),
                    });
                }
            }
        }
    }
    Ok(found.into_iter().collect())
}

pub fn valid(value: &str) -> bool {
    let (claim, named, soft) = fields(value);
    let source = named.get("origin").copied().unwrap_or("");
    !claim.is_empty()
        && ["pointer", "rationale", "revisit"]
            .iter()
            .all(|k| named.get(k).is_some_and(|v| !v.is_empty()))
        && (origin(source) || source == "none recorded" || soft && source == "none")
}

pub fn claims(repo: &Path, rev: &str) -> Result<Vec<String>, String> {
    let mut text = git(repo, &["show", &format!("{rev}:{RECORDS}")]).unwrap_or_default();
    text.push('\n');
    text.push_str(&git(repo, &["log", "--format=%B", rev])?);
    Ok(records(&text, "Provenance:")
        .into_iter()
        .filter(|v| valid(v))
        .map(|v| rule_text(fields(v).0))
        .collect())
}

pub fn covered(item: &Item, claims: &[String]) -> bool {
    claims.iter().any(|c| c.contains(&item.key))
}

fn pointer(repo: &Path, rev: &str, item: &Item) -> String {
    let found = if item.needle.is_empty() {
        git(repo, &["log", "--follow", "--diff-filter=A", "--format=%h %as", rev, "--", &item.path])
    } else {
        git(repo, &["log", "--follow", "--format=%h %as", "-S", &item.needle, rev, "--", &item.path])
    };
    match found.ok().and_then(|log| log.lines().last().map(str::to_owned)) {
        Some(first) => format!("git log -S first adds it in {first}"),
        None => "git log -S finds no commit".into(),
    }
}

pub fn stub(repo: &Path, rev: &str, item: &Item) -> String {
    let claim = if item.key == item.path {
        format!("{} {}", item.kind.name(), item.path)
    } else {
        format!("{} {}: {}", item.kind.name(), item.path, item.key)
    };
    format!(
        "Provenance: {claim}; origin none recorded; pointer {}; rationale not recorded; revisit when this {} next changes",
        pointer(repo, rev, item),
        item.kind.name()
    )
}

pub fn inventory(repo: &Path, rev: &str, stubs: bool) -> Result<u8, String> {
    let found = items(repo, rev)?;
    let claims = claims(repo, rev)?;
    let missing: Vec<_> = found.iter().filter(|i| !covered(i, &claims)).collect();
    for item in &missing {
        if stubs {
            println!("{}", stub(repo, rev, item));
        } else {
            println!("{}\t{}\t{}", item.kind.name(), item.path, item.key);
        }
    }
    let count = |kind| found.iter().filter(|i| i.kind == kind).count();
    eprintln!(
        "provenance inventory: {} items ({} hooks, {} policy lines, {} tiers, {} gating bars), {} without a record",
        found.len(),
        count(Kind::Hook),
        count(Kind::Policy),
        count(Kind::Tier),
        count(Kind::Bar),
        missing.len()
    );
    Ok(u8::from(!missing.is_empty()))
}
