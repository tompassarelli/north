use std::io::Write;
use std::path::Path;

use north_base::foundation::offload::{ONE_LANE_ENV, TIPS_ENV};

use crate::{Range, git, holdlist, lanes};

const SELF: &str = "turn-scope";
const TURNS: &str = "docs/rationale/discovery-turns.toml";
const EXPERIMENTS: &str = "docs/rationale/experiments.toml";

fn turns(text: Option<String>) -> Vec<String> {
    format!("\n{}", text.unwrap_or_default()).split("\n[[turn]]").skip(1).map(|turn| turn.trim().to_owned()).collect()
}

fn changed(directory: &Path, fork: &str, head: &str) -> Vec<(String, u64)> {
    git::text(directory, &["diff", "--numstat", "--no-renames", fork, head])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let _added = fields.next()?;
            let removed = fields.next()?.parse().unwrap_or(u64::MAX);
            Some((fields.next()?.to_owned(), removed))
        })
        .collect()
}

pub fn check(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let show = |revision: &str| git::text(directory, &["show", &format!("{revision}:{TURNS}")]);
    let (before, after) = (turns(show(&fork)), turns(show(head)));
    let mut refusals = Vec::new();
    if before.iter().enumerate().any(|(index, turn)| after.get(index) != Some(turn)) {
        refusals.push(format!("the lane changes or removes a recorded [[turn]] of {TURNS}"));
    }
    if after.len() > before.len() {
        for (path, removed) in changed(directory, &fork, head) {
            match path.as_str() {
                TURNS => {}
                EXPERIMENTS if removed == 0 => {}
                EXPERIMENTS => refusals.push(format!("the lane adds a [[turn]] and changes or removes lines of {EXPERIMENTS}, so a bar, window or review day may move")),
                other => refusals.push(format!("the lane adds a [[turn]] and also changes {other}, outside the turn's scope of {TURNS} and added lines of {EXPERIMENTS}")),
            }
        }
    }
    match refusals.is_empty() {
        true => Ok(()),
        false => Err(refusals.join("\n")),
    }
}

pub fn judge(directory: &Path, range: &str, tips: &str, whole: bool) -> Result<(), String> {
    let parts = lanes::ranges(directory, range, tips, whole).unwrap_or_else(|| vec![range.to_owned()]);
    let refusals: Vec<String> = parts
        .iter()
        .filter_map(|part| {
            let Range { base, head } = Range::parse(part)?;
            let refusal = check(directory, base, head).err()?;
            Some(if parts.len() > 1 { refusal.lines().map(|line| format!("{part}: {line}")).collect::<Vec<_>>().join("\n") } else { refusal })
        })
        .collect();
    match refusals.is_empty() {
        true => Ok(()),
        false => Err(refusals.join("\n")),
    }
}

pub fn scan(directory: &Path, range: &str, state: &Path, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let tips = std::env::var(TIPS_ENV).unwrap_or_default();
    let whole = std::env::var(ONE_LANE_ENV).is_ok_and(|flag| flag == "1");
    let outcome = judge(directory, range, &tips, whole);
    let enforce = holdlist::enforcing(directory, SELF, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        holdlist::record(state, SELF, head, outcome.as_ref().err().map(String::as_str));
    }
    let Err(refusal) = outcome else {
        return 0;
    };
    let verb = if enforce { "refused" } else { "would refuse (shadow)" };
    let _ = writeln!(out, "{SELF}: {verb}: the lane steps outside a mechanism-discovery turn's scope:");
    for line in refusal.lines() {
        let _ = writeln!(out, "  {line}");
    }
    u8::from(enforce)
}

#[cfg(test)]
#[path = "turn_scope_tests.rs"]
mod tests;
