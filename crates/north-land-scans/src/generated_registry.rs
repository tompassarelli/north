use std::io::Write;
use std::path::Path;

use north_landing_gates::landing::generated_registry::{refusal, registries};

use super::{Range, git};

pub const USAGE: &str = "usage: north land scan generated-registry BASE..HEAD";

pub fn check(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let diff = git::text(directory, &["diff", "--name-only", "--no-renames", "--diff-filter=d", &fork, head, "--"]).ok_or_else(|| format!("cannot diff {fork}..{head}"))?;
    let changed: Vec<String> = diff.lines().map(str::to_owned).collect();
    refusal(&registries(), &changed)
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

