//! `north land scan gate-shadow BASE..HEAD`: a lane may not add a gate that refuses from its first push, nor make a
//! gate enforce without the shadow record behind it. A file the lane adds under a gate directory must be named by a
//! gate entry in harness/gates.toml, and a gate the lane adds as `enforce` or switches from `shadow` to `enforce` must
//! carry a review that read at least 24 h or 20 of its decisions and found no wrong refusal.
use std::io::Write;
use std::path::Path;

use north_base::gates::{self, DIRECTORIES, Gate, Mode};

use super::{Range, git};

pub const ID: &str = "gate-shadow";

fn registry(directory: &Path, revision: &str) -> Result<Vec<Gate>, String> {
    git::text(directory, &["show", &format!("{revision}:{}", gates::PATH)]).map_or_else(|| Ok(Vec::new()), |text| gates::parse(&text))
}

fn gate_file(path: &str) -> bool {
    path.ends_with(".rs") && DIRECTORIES.iter().any(|directory| path.starts_with(directory)) && !path.contains("test") && !path.contains("/fixtures/")
}

/// What the lane would be refused for, one reason per gate or file.
pub fn findings(directory: &Path, base: &str, head: &str, now: i64) -> Result<Vec<String>, String> {
    let (before, after) = (registry(directory, base)?, registry(directory, head)?);
    let mut found = Vec::new();
    for gate in after.iter().filter(|gate| gate.mode == Mode::Enforce) {
        if before.iter().any(|old| old.id == gate.id && old.mode == Mode::Enforce) {
            continue;
        }
        let short = gate.review.as_ref().map_or_else(|| Some("it has no [gate.review] shadow record".to_owned()), |review| review.short(now));
        if let Some(why) = short {
            found.push(format!("{} enforces without its shadow record: {why}; land it as mode = \"shadow\" first", gate.id));
        }
    }
    let added = git::text(directory, &["diff", "--name-only", "--diff-filter=A", &format!("{base}..{head}")]).ok_or_else(|| format!("cannot list the files {base}..{head} adds"))?;
    for path in added.lines().filter(|path| gate_file(path)) {
        if !after.iter().any(|gate| gate.files.iter().any(|file| file == path)) {
            found.push(format!("{path} adds a gate no entry of {} names; register it there with mode = \"shadow\"", gates::PATH));
        }
    }
    Ok(found)
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let base = git::merge_base(directory, base, head);
    let now = north_base::foundation::clock::Timestamp::now().unix_seconds();
    match findings(directory, &base, head, now) {
        Err(why) => {
            let _ = writeln!(out, "{ID}: {why}");
            1
        }
        Ok(found) => {
            let refusal = (!found.is_empty()).then(|| found.join("; "));
            match gates::decide(ID, refusal) {
                Some(refusal) => {
                    let _ = writeln!(out, "{ID}: {refusal}");
                    1
                }
                None => 0,
            }
        }
    }
}
