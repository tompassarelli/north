use std::io::Write;
use std::path::Path;

use north_landing_gates::landing::merge_set::{self, Kind};

use super::{Range, git};

pub const USAGE: &str = "usage: north land scan generated-registry BASE..HEAD";

/// A generated registry is written at land time from its per-unit sources. A lane that commits
/// the generated file by hand, or regenerates it itself, is refused so the file never enters a
/// worker's diff and the lander's refresh owns the only copy.
pub fn check(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = directory.join(merge_set::DEFAULT_CONFIG);
    if !config.exists() {
        return Ok(());
    }
    let rules = merge_set::read_rules(&config)?;
    let fork = git::merge_base(directory, base, head);
    let diff = git::text(directory, &["diff", "--name-only", "--no-renames", &fork, head, "--"]).ok_or_else(|| format!("cannot diff {fork}..{head}"))?;
    let changed: Vec<String> = diff.lines().map(str::to_owned).collect();
    for rule in rules {
        let Kind::Regenerate { sources, .. } = &rule.kind else { continue };
        if !changed.contains(&rule.path) {
            continue;
        }
        let source = changed.iter().find(|path| sources.iter().any(|prefix| path.starts_with(prefix.as_str())));
        let edit = source.map_or_else(|| format!("a file under {}", sources.join(", ")), |path| path.clone());
        return Err(format!(
            "{} is generated from {}; edit {edit} instead, remove {} from the commit, and let the lander write it at land time; NOT queued.",
            rule.path,
            sources.join(", "),
            rule.path
        ));
    }
    Ok(())
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "generated-registry: expected BASE..HEAD, got {range}");
        return 2;
    };
    match check(directory, base, head) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(out, "{error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "generated_registry_tests.rs"]
mod tests;

