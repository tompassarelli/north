use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use super::mutant::{Config, Env, Stop, git, git_bytes, git_piped, git_text, repo_name, valid_id};
use super::mutant_trees::{self, Outcome, Plan};

static TRAILER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Test-kind:\s+(.+)\s+[1-5]\s+kills\s+(\S+)\s*$").unwrap());

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Claim {
    pub(super) test: String,
    pub(super) mutant: String,
}

pub(super) fn verify(cwd: &Path, base: &str, head: &str, env: &Env, out: &mut dyn Write) -> Result<(), Stop> {
    let claims = claims(cwd, base, head)?;
    if claims.is_empty() {
        return Ok(());
    }
    let repo = repo_name(cwd)?;
    let patches = recorded(cwd, &claims, base, head, env, &repo, out)?;
    let Some(config) = config(cwd, base)? else {
        return Ok(());
    };
    let change = change_id(cwd, base, head)?;
    let kills = env.mutants(&repo).join("kills");
    let mut pending = Vec::new();
    for claim in claims {
        let marker = kills.join(kill_key(&change, &patches[&claim.mutant], &config.run, &claim.test));
        if marker.is_file() {
            writeln!(out, "mutant: ✓ {} kills {} (cached)", claim.test, claim.mutant)?;
        } else {
            pending.push((claim, marker));
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    let plan = Plan {
        head,
        mutants: pending.iter().map(|(claim, _)| (claim.mutant.clone(), env.patch(&repo, &claim.mutant))).collect(),
        tests: pending.iter().map(|(claim, _)| claim.test.clone()).collect(),
        config: &config,
        repo: &repo,
    };
    let outcome = mutant_trees::run(cwd, &plan, env, out)?;
    judge(&pending, &outcome, out)
}

fn claims(cwd: &Path, base: &str, head: &str) -> Result<BTreeSet<Claim>, Stop> {
    let log = git_bytes(cwd, None, &["log", "--format=%B", &format!("{base}..{head}")])?;
    Ok(String::from_utf8_lossy(&log)
        .lines()
        .filter_map(|line| TRAILER.captures(line))
        .map(|found| Claim { test: found[1].to_owned(), mutant: found[2].to_owned() })
        .collect())
}

fn recorded(
    cwd: &Path,
    claims: &BTreeSet<Claim>,
    base: &str,
    head: &str,
    env: &Env,
    repo: &str,
    out: &mut dyn Write,
) -> Result<BTreeMap<String, Vec<u8>>, Stop> {
    let scratch = tempfile::tempdir()?;
    let index = scratch.path().join("index");
    let mut patches = BTreeMap::new();
    let mut refused = false;
    for claim in claims {
        if !patches.contains_key(&claim.mutant) {
            let patch = valid_id(&claim.mutant).then(|| env.patch(repo, &claim.mutant));
            let bytes = patch.as_ref().and_then(|patch| fs::read(patch).ok()).unwrap_or_default();
            let applies = !bytes.is_empty()
                && patch.is_some_and(|patch| [base, head].iter().any(|tree| applies(cwd, &index, tree, &patch)));
            patches.insert(claim.mutant.clone(), applies.then_some(bytes));
        }
        if patches[&claim.mutant].is_none() {
            writeln!(out, "safe-push: ✗ Test-kind {} claims mutant {}, which has no recorded non-empty patch; record it with mutant record; NOT pushed.", claim.test, claim.mutant)?;
            refused = true;
        }
    }
    if refused {
        return Err(Stop::Refused);
    }
    Ok(patches.into_iter().filter_map(|(id, bytes)| Some((id, bytes?))).collect())
}

fn applies(cwd: &Path, index: &Path, tree: &str, patch: &Path) -> bool {
    let succeeded = |args: &[&str]| git(cwd, Some(index), args).is_ok_and(|output| output.status.success());
    succeeded(&["read-tree", tree]) && succeeded(&["apply", "--cached", "--check", &patch.to_string_lossy()])
}

fn config(cwd: &Path, base: &str) -> Result<Option<Config>, Stop> {
    let blob = format!("{base}:.safe-push");
    if !git(cwd, None, &["cat-file", "-e", &blob])?.status.success() {
        return Ok(None);
    }
    let get = |args: &[&str]| -> Result<Vec<String>, Stop> {
        let output = git(cwd, None, &[&["config", "--blob", &blob], args].concat())?;
        match output.status.code() {
            Some(0) => Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()),
            Some(1) => Ok(Vec::new()),
            _ => Err(Stop::Fail(format!("mutant: cannot read {blob}: {}", String::from_utf8_lossy(&output.stderr).trim()))),
        }
    };
    let Some(run) = get(&["--get", "mutant.run"])?.pop().filter(|run| !run.trim().is_empty()) else {
        return Ok(None);
    };
    let class = get(&["--get", "mutant.class"])?.pop().unwrap_or_else(|| "moderate".to_owned());
    if class != "moderate" && class != "heavy" {
        return Err(Stop::Fail(format!("mutant: mutant.class in {blob} must be moderate or heavy, not {class}")));
    }
    Ok(Some(Config { run, class, links: get(&["--get-all", "mutant.link"])? }))
}

fn change_id(cwd: &Path, base: &str, head: &str) -> Result<String, Stop> {
    let diff = git_bytes(cwd, None, &["diff", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", base, head])?;
    if diff.is_empty() {
        return git_text(cwd, None, &["rev-parse", &format!("{head}^{{tree}}")]);
    }
    let output = git_piped(cwd, &["patch-id", "--verbatim"], &diff)?;
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().next() {
        Some(id) if output.status.success() => Ok(id.to_owned()),
        _ => Err(Stop::Fail(format!("mutant: git patch-id failed: {}", String::from_utf8_lossy(&output.stderr).trim()))),
    }
}

fn kill_key(change: &str, patch: &[u8], run: &str, test: &str) -> String {
    let patch = format!("{:x}", Sha256::digest(patch));
    format!("{:x}", Sha256::digest([change, &patch, run, test].join("\0")))
}

fn judge(pending: &[(Claim, PathBuf)], outcome: &Outcome, out: &mut dyn Write) -> Result<(), Stop> {
    let mut refused = false;
    for (claim, marker) in pending {
        let mutated = outcome.mutants.get(&claim.mutant).and_then(|results| results.get(&claim.test));
        if outcome.change.get(&claim.test) != Some(&true) {
            writeln!(out, "safe-push: ✗ Test-kind {} fails or did not run on the change itself, so it cannot vouch for mutant {}; NOT pushed.", claim.test, claim.mutant)?;
            refused = true;
        } else if mutated != Some(&false) {
            writeln!(out, "safe-push: ✗ Test-kind {} does not kill mutant {}: it passes (or did not run) with the mutant applied; NOT pushed.", claim.test, claim.mutant)?;
            refused = true;
        } else {
            if let Some(kills) = marker.parent() {
                fs::create_dir_all(kills)?;
            }
            fs::write(marker, b"")?;
            writeln!(out, "mutant: ✓ {} kills {} ({:.1} s)", claim.test, claim.mutant, outcome.seconds)?;
        }
    }
    if refused {
        writeln!(out, "mutant: runner output ends:\n{}", outcome.tail)?;
        return Err(Stop::Refused);
    }
    Ok(())
}
