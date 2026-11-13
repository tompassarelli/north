use super::lock::{age_violations, flake_owners, lock_candidates, owner_violations};
use super::manifest::{parse_manifests, violations};
use super::registry::{PublicRegistries, Registry, publish_times};
use super::snapshot::{Snapshot, git, read_snapshot};
use super::trailer::{dependency_reasons, urgent_dependencies};
use super::{TRAILER_FORMAT, URGENT_FORMAT, Urgent};
use crate::foundation::clock::Timestamp;
use std::collections::BTreeMap;
use std::path::Path;

struct Reviewed {
    base: Snapshot,
    head: Snapshot,
    messages: Vec<String>,
    failures: Vec<String>,
}

fn review_reasons(root: &Path, args: &[String]) -> Result<Reviewed, (u8, String)> {
    if !git(root, &["rev-parse", "--show-prefix"])
        .map_err(|e| (1, e))?
        .trim()
        .is_empty()
    {
        return Err((1, "run dep-gate from the repository root".into()));
    }
    let revisions: Vec<_> = args
        .iter()
        .map(|rev| {
            if rev == crate::language_gate::EMPTY_TREE {
                return Ok(rev.clone());
            }
            git(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{rev}^{{commit}}"),
                ],
            )
            .map(|s| s.trim().to_owned())
        })
        .collect::<Result<_, _>>()
        .map_err(|e| (1, e))?;
    let (base, head) = (&revisions[0], &revisions[1]);
    let base_snapshot = read_snapshot(root, base).map_err(|e| (1, e))?;
    let head_snapshot = read_snapshot(root, head).map_err(|e| (1, e))?;
    let before = parse_manifests(&base_snapshot.manifests).map_err(|e| (1, e))?;
    let after = parse_manifests(&head_snapshot.manifests).map_err(|e| (1, e))?;
    let messages: Vec<_> = git(
        root,
        &["log", "--format=%B%x00", &format!("{base}..{head}")],
    )
    .map_err(|e| (1, e))?
    .split('\0')
    .map(str::to_owned)
    .collect();
    let mut failures = violations(&before, &after, &dependency_reasons(&messages));
    let candidates = lock_candidates(&base_snapshot.locks, &head_snapshot.locks).map_err(|error| (1, error))?;
    failures.extend(owner_violations(&candidates, &flake_owners(base_snapshot.settings.as_deref().unwrap_or_default())));
    Ok(Reviewed { base: base_snapshot, head: head_snapshot, messages, failures })
}

/// The offline half of dep-gate: every added dependency names a reason and comes from an allowed source; it never reads a registry.
pub fn check_reasons(root: &Path, base: &str, head: &str) -> bool {
    match review_reasons(root, &[base.to_owned(), head.to_owned()]) {
        Ok(reviewed) if reviewed.failures.is_empty() => true,
        Ok(reviewed) => {
            println!("{}\n{TRAILER_FORMAT}", reviewed.failures.join("\n"));
            false
        }
        Err((_, error)) => {
            eprintln!("dep-gate: {error}");
            false
        }
    }
}

fn young(locks: (&BTreeMap<String, String>, &BTreeMap<String, String>), urgent: &Urgent, registry: &dyn Registry) -> Result<Vec<String>, (u8, String)> {
    let candidates = lock_candidates(locks.0, locks.1).map_err(|e| (1, e))?;
    let times = publish_times(&candidates, urgent, registry).map_err(|e| (2, e))?;
    Ok(age_violations(&candidates, &times, Timestamp::now().unix_seconds_f64(), urgent))
}

fn evaluate(root: &Path, args: &[String]) -> Result<(u8, String), (u8, String)> {
    let reviewed = review_reasons(root, args)?;
    if !reviewed.failures.is_empty() {
        return Ok((1, format!("{}\n{TRAILER_FORMAT}\n", reviewed.failures.join("\n"))));
    }
    let failures = young((&reviewed.base.locks, &reviewed.head.locks), &urgent_dependencies(&reviewed.messages), &PublicRegistries)?;
    if !failures.is_empty() {
        return Ok((1, format!("{}\n{URGENT_FORMAT}\n", failures.join("\n"))));
    }
    Ok((0, String::new()))
}

/// The lander's half of the 7-day floor: whether a lock entry the lane adds was published under 7 days ago with no `Dependency-Urgent:` trailer, read from `registry` on the judging host.
pub fn refuse_young(root: &Path, base: &str, head: &str, registry: &dyn Registry, out: &mut impl std::io::Write) -> bool {
    let verdict = review_reasons(root, &[base.to_owned(), head.to_owned()]).and_then(|reviewed| young((&reviewed.base.locks, &reviewed.head.locks), &urgent_dependencies(&reviewed.messages), registry));
    match verdict {
        Ok(failures) if failures.is_empty() => false,
        Ok(failures) => {
            let _ = writeln!(out, "{}\n{URGENT_FORMAT}", failures.join("\n"));
            true
        }
        Err((_, error)) => {
            let _ = writeln!(out, "dep-gate: {error}");
            true
        }
    }
}

/// Puts `lock` back to `before` when it now holds an entry published under 7 days ago that `before` lacked, or when the registry cannot say; returns the entries that restored it.
pub fn hold_floor(lock: &Path, before: &str, registry: &dyn Registry) -> Result<Vec<String>, String> {
    let after = std::fs::read_to_string(lock).unwrap_or_default();
    let name = lock.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let locks = (BTreeMap::from([(name.clone(), before.to_owned())]), BTreeMap::from([(name, after)]));
    let young = young((&locks.0, &locks.1), &Urgent::new(), registry).unwrap_or_else(|(_, error)| vec![format!("dep-gate: {error}")]);
    if !young.is_empty() {
        std::fs::write(lock, before).map_err(|error| format!("{}: {error}", lock.display()))?;
    }
    Ok(young)
}

/// `north land scan dep-floor LOCK`, with the lock as it was before a cargo command on stdin: `north depbox fetch` runs it whenever cargo rewrote Cargo.lock, so `cargo update` resolves under the 7-day floor.
pub fn floor_run(lock: &Path) -> u8 {
    let mut before = String::new();
    if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut before) {
        eprintln!("dep-floor: stdin: {error}");
        return 1;
    }
    match hold_floor(lock, &before, &PublicRegistries) {
        Ok(young) if young.is_empty() => 0,
        Ok(young) => {
            eprintln!("{}\ndep-floor: restored {} as it was before the command, since it resolved the entries above under the 7-day floor; pin each to an older version (cargo update -p NAME --precise VERSION)", young.join("\n"), lock.display());
            1
        }
        Err(error) => {
            eprintln!("dep-floor: {error}");
            1
        }
    }
}

pub fn run(args: &[String]) -> u8 {
    if args.len() != 2 {
        eprintln!("usage: dep-gate BASE HEAD\n{URGENT_FORMAT}");
        return 1;
    }
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("dep-gate: {e}");
            return 1;
        }
    };
    report(evaluate(&root, args))
}

pub fn run_at(root: &Path, base: &str, head: &str) -> u8 {
    report(evaluate(root, &[base.to_owned(), head.to_owned()]))
}

fn report(verdict: Result<(u8, String), (u8, String)>) -> u8 {
    match verdict {
        Ok((code, output)) => {
            print!("{output}");
            code
        }
        Err((code, error)) => {
            eprintln!("dep-gate: {error}");
            code
        }
    }
}
