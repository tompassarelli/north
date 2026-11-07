mod snapshot;
#[cfg(test)]
mod tests;
mod workspace;

use super::history::allows;
use snapshot::Snapshot;
use std::path::PathBuf;
use workspace::{Caps, Measured, census, owns};

const USAGE: &str = "usage: north code-health crate-lines [--repo DIR] [BASE HEAD]";

pub(super) fn run(arguments: &[String]) -> u8 {
    let (repo, range) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (PathBuf::from(dir), rest),
        rest => (PathBuf::from("."), rest),
    };
    let outcome = match range {
        [] => Snapshot::worktree(&repo).and_then(|head| report(&head)).map(|lines| (lines, 0)),
        [base, head] => gate(&repo, base, head),
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok((lines, code)) => {
            for line in lines {
                println!("{line}");
            }
            code
        }
        Err(error) => {
            eprintln!("north code-health crate-lines: {error}");
            2
        }
    }
}

fn gate(repo: &std::path::Path, base: &str, head: &str) -> Result<(Vec<String>, u8), String> {
    let allowed = allows(repo, &[&format!("{base}..{head}")])?;
    let (crossed, grew): (Vec<Growth>, Vec<Growth>) = growth(&Snapshot::at(repo, base)?, &Snapshot::at(repo, head)?)?.into_iter().partition(|growth| growth.crossed);
    let refused: Vec<String> = grew.into_iter().filter(|growth| !allowed.iter().any(|allow| allow.rule == "crate-lines" && allow.path == growth.name)).map(|growth| growth.line).collect();
    let code = u8::from(!refused.is_empty());
    let summary = if refused.is_empty() { "crate-lines: no crate over its cap grew".to_owned() } else { format!("✗ crate-lines: {} crate(s) over their cap grew", refused.len()) };
    Ok((crossed.into_iter().map(|growth| growth.line).chain(refused).chain([summary]).collect(), code))
}

pub(super) fn lines_at(repo: &std::path::Path, rev: &str) -> Result<Vec<String>, String> {
    report(&Snapshot::at(repo, rev)?)
}

fn report(head: &Snapshot) -> Result<Vec<String>, String> {
    let caps = Caps::read(head)?;
    let mut crates = census(head, &caps);
    crates.sort_by(|left, right| right.lines.cmp(&left.lines).then_with(|| left.name.cmp(&right.name)));
    let width = crates.iter().map(|krate| krate.name.len()).max().unwrap_or(0);
    Ok(crates
        .iter()
        .map(|krate| {
            let status = if krate.lines > krate.cap { format!("over by {}", krate.lines - krate.cap) } else { "within".to_owned() };
            format!("crate-lines: {:width$} {:>7} non-test lines, cap {:>6} ({} dependents), {status}", krate.name, krate.lines, krate.cap, krate.dependents)
        })
        .collect())
}

struct Growth {
    name: String,
    crossed: bool,
    line: String,
}

fn growth(base: &Snapshot, head: &Snapshot) -> Result<Vec<Growth>, String> {
    let caps = Caps::read(head)?;
    let before = census(base, &caps);
    let after = census(head, &caps);
    Ok(after
        .iter()
        .filter(|krate| krate.lines > krate.cap)
        .filter_map(|krate| {
            let previous = before.iter().find(|old| old.name == krate.name);
            let (grew, crossed) = match previous {
                None => (format!("{} is a new crate with {} non-test lines", krate.name, krate.lines), true),
                Some(old) if krate.lines > old.lines => (format!("{} grew {} -> {} non-test lines", krate.name, old.lines, krate.lines), old.lines <= krate.cap),
                Some(_) => return None,
            };
            let destination = destination(krate, previous, &after);
            let line = if crossed {
                format!("crate-lines: {grew}, crossing its cap of {} ({} dependents); it lands, the crate-headroom detector files its split box, and growth past the cap is refused until it is split: put new code in {destination}", krate.cap, krate.dependents)
            } else {
                format!("✗ crate-lines: {grew}, over its cap of {} ({} dependents); put the new code in {destination}, or record why in a `Code-health-allow: crate-lines {} — <reason>` trailer", krate.cap, krate.dependents, krate.name)
            };
            Some(Growth { name: krate.name.clone(), crossed, line })
        })
        .collect())
}

fn destination(krate: &Measured, previous: Option<&Measured>, crates: &[Measured]) -> String {
    let roomy = crates.iter().filter(|other| krate.depends_on.contains(&other.name) && other.lines < other.cap).min_by_key(|other| other.lines);
    match roomy {
        Some(other) => format!("{} ({}/, {} of {} lines), which {} already depends on", other.name, other.dir, other.lines, other.cap, krate.name),
        None => format!("a new crate crates/{}/ that {} depends on", new_crate(krate, previous), krate.name),
    }
}

fn new_crate(krate: &Measured, previous: Option<&Measured>) -> String {
    let grown = krate
        .files
        .iter()
        .map(|(path, lines)| (path, lines.saturating_sub(previous.and_then(|old| old.files.get(path)).copied().unwrap_or(0))))
        .max_by_key(|(_, growth)| *growth)
        .map(|(path, _)| path.as_str());
    let source = if krate.dir.is_empty() { "src/".to_owned() } else { format!("{}/src/", krate.dir) };
    let module = grown.and_then(|path| path.strip_prefix(&source)).and_then(|rest| rest.split('/').next()).map(|first| first.trim_end_matches(".rs")).filter(|module| !matches!(*module, "lib" | "main"));
    let family = krate.name.split('-').next().unwrap_or(&krate.name);
    match module {
        Some(module) => format!("{family}-{}", module.replace('_', "-")),
        None => format!("{}-split", krate.name),
    }
}

const NEAR_CAP_PERCENT: usize = 95;
const SPLIT_TARGET_PERCENT: usize = 60;

pub(super) fn headroom(repo: &std::path::Path, edited: &str) -> Option<String> {
    let head = Snapshot::worktree(repo).ok()?;
    let caps = Caps::read(&head).ok().filter(|caps| caps.enrolled)?;
    let crates = census(&head, &caps);
    let krate = crates.iter().filter(|krate| owns(&krate.dir, edited)).max_by_key(|krate| krate.dir.len())?;
    let (destination, target) = (destination(krate, Some(krate), &crates), krate.cap * SPLIT_TARGET_PERCENT / 100);
    if krate.lines > krate.cap {
        return Some(format!("crate-lines: {} is over its cap ({} of {} non-test lines), so a landing that grows it is refused; put new code in {destination}; its split to about {target} lines is a box on the After-landing gate debt issue", krate.name, krate.lines, krate.cap));
    }
    (krate.lines * 100 >= krate.cap * NEAR_CAP_PERCENT).then(|| format!("crate-lines: {} has {} lines left under its {}-line cap; this lane may cross it, and new code fits better in {destination}; its split to about {target} lines is a box on the After-landing gate debt issue", krate.name, krate.cap - krate.lines, krate.cap))
}
