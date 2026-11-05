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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run(root, &["init", "-q", "-b", "main"]);
        write(root, "harness/hooks/guard.sh", "#!/bin/sh\n");
        write(root, "harness/hooks/guard.test.sh", "#!/bin/sh\n");
        write(root, "harness/claude/agents/worker.md", "worker\n");
        write(
            root,
            "AGENTS.md",
            "# Rules\n\n- Run the [tests](docs/t.md); then land.\n\n## Routers\n\n- Before X, read Y.\n",
        );
        write(
            root,
            "docs/plan.md",
            "- [ ] the build stays under 30 s\n- [x] measured 12 s\nDone when: replies within 2 s\n",
        );
        write(root, "docs/fixtures/plan.md", "- [ ] under 9 s\n");
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", "fixture"]);
        dir
    }

    #[test]
    fn the_inventory_finds_each_hook_policy_line_tier_and_gating_bar_on_main() {
        let dir = fixture();
        let found: Vec<_> = items(dir.path(), "HEAD")
            .unwrap()
            .into_iter()
            .map(|i| (i.kind, i.path, i.key))
            .collect();
        assert_eq!(
            found,
            vec![
                (Kind::Hook, "harness/hooks/guard.sh".into(), "harness/hooks/guard.sh".into()),
                (Kind::Policy, "AGENTS.md".into(), "Run the tests at docs/t.md, then land.".into()),
                (Kind::Tier, "harness/claude/agents/worker.md".into(), "harness/claude/agents/worker.md".into()),
                (Kind::Bar, "docs/plan.md".into(), "Done when: replies within 2 s".into()),
                (Kind::Bar, "docs/plan.md".into(), "[ ] the build stays under 30 s".into()),
            ]
        );
    }

    #[test]
    fn a_none_recorded_record_in_the_records_file_counts_and_an_item_without_one_does_not() {
        let dir = fixture();
        let root = dir.path();
        assert_eq!(inventory(root, "HEAD", false), Ok(1));
        let item = items(root, "HEAD").unwrap().remove(0);
        let line = stub(root, "HEAD", &item);
        assert!(line.contains("; origin none recorded; pointer git log -S first adds it in "), "{line}");
        let mut text: String = items(root, "HEAD")
            .unwrap()
            .iter()
            .map(|i| stub(root, "HEAD", i) + "\n")
            .collect();
        write(root, RECORDS, &text);
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", "records"]);
        assert_eq!(inventory(root, "HEAD", false), Ok(0));
        text = text.replace("; origin none recorded", "; origin invented");
        write(root, RECORDS, &text);
        run(root, &["commit", "-qam", "bad origins"]);
        assert_eq!(inventory(root, "HEAD", false), Ok(1));
        let claims = claims(root, "HEAD").unwrap();
        assert!(claims.is_empty());
    }

    #[test]
    fn a_commit_trailer_on_main_records_its_item() {
        let dir = fixture();
        let root = dir.path();
        run(
            root,
            &[
                "commit", "-q", "--allow-empty", "-m",
                "Add the guard\n\nProvenance: hook harness/hooks/guard.sh; origin incident; pointer north#1; rationale not recorded; revisit 2027-01-01",
            ],
        );
        let claims = claims(root, "HEAD").unwrap();
        let found = items(root, "HEAD").unwrap();
        let hook = found.iter().find(|i| i.kind == Kind::Hook).unwrap();
        let tier = found.iter().find(|i| i.kind == Kind::Tier).unwrap();
        assert!(covered(hook, &claims));
        assert!(!covered(tier, &claims));
    }

    #[test]
    fn a_stub_points_at_the_commit_that_first_added_the_text_across_a_rename() {
        let dir = fixture();
        let root = dir.path();
        let first = git(root, &["log", "-1", "--format=%h", "HEAD"]).unwrap();
        run(root, &["mv", "AGENTS.md", "harness/AGENTS.md"]);
        run(root, &["mv", "harness/claude/agents/worker.md", "harness/claude/agents/lead.md"]);
        run(root, &["commit", "-q", "-m", "rename"]);
        let found = items(root, "HEAD").unwrap();
        for item in found.iter().filter(|i| matches!(i.kind, Kind::Policy | Kind::Tier)) {
            let line = stub(root, "HEAD", item);
            assert!(line.contains(&format!("first adds it in {} ", first.trim())), "{line}");
        }
    }
}
