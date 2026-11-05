mod snapshot;
mod workspace;
#[cfg(test)]
mod tests;

use snapshot::Snapshot;
use std::path::PathBuf;
use workspace::{Caps, Measured, census};

const USAGE: &str = "usage: north code-health crate-lines [--repo DIR] [BASE HEAD]";

pub(super) fn run(arguments: &[String]) -> u8 {
    let (repo, range) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (PathBuf::from(dir), rest),
        rest => (PathBuf::from("."), rest),
    };
    let outcome = match range {
        [] => Snapshot::worktree(&repo).and_then(|head| report(&head)).map(|lines| (lines, 0)),
        [base, head] => gate(&Snapshot::at(&repo, base), &Snapshot::at(&repo, head)),
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

fn gate(base: &Result<Snapshot, String>, head: &Result<Snapshot, String>) -> Result<(Vec<String>, u8), String> {
    let (base, head) = (base.as_ref()?, head.as_ref()?);
    let refused = refusals(base, head)?;
    let code = u8::from(!refused.is_empty());
    let summary = if refused.is_empty() { "crate-lines: no crate over its cap grew".to_owned() } else { format!("✗ crate-lines: {} crate(s) over their cap grew", refused.len()) };
    Ok((refused.into_iter().chain([summary]).collect(), code))
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

fn refusals(base: &Snapshot, head: &Snapshot) -> Result<Vec<String>, String> {
    let caps = Caps::read(head)?;
    let before = census(base, &caps);
    let after = census(head, &caps);
    Ok(after
        .iter()
        .filter(|krate| krate.lines > krate.cap)
        .filter_map(|krate| {
            let previous = before.iter().find(|old| old.name == krate.name);
            let grew = match previous {
                None => format!("{} is a new crate with {} non-test lines", krate.name, krate.lines),
                Some(old) if krate.lines > old.lines => format!("{} grew {} -> {} non-test lines", krate.name, old.lines, krate.lines),
                Some(_) => return None,
            };
            Some(format!("✗ crate-lines: {grew}, over its cap of {} ({} dependents); put the new code in {}", krate.cap, krate.dependents, destination(krate, previous, &after)))
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
