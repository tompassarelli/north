use crate::guard::host::{self, ReadStreak};
use crate::guard::proxy_role;
use crate::hook::{HookInput, Verdict};
use std::io::{Read, Seek, SeekFrom};

const UNIT: &str = "proxy-read-streak";
const LIMIT: &str = "proxy_read_streak";
const TAIL_BYTES: u64 = 8 * 1024 * 1024;

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") || !proxy_role::is_proxy(&proxy_role::Live, input) {
        return Verdict::Allow;
    }
    let Some(transcript) = input.data["transcript_path"].as_str().and_then(tail) else { return Verdict::Allow };
    let Ok(hook) = serde_json::from_slice::<serde_json::Value>(&input.raw) else { return Verdict::Allow };
    host::read_streak(&transcript, &hook).map_or(Verdict::Allow, |streak| verdict(&streak, limit()))
}

pub fn limit() -> usize {
    north_base::platform::harness::default_number(LIMIT) as usize
}

pub fn verdict(streak: &ReadStreak, limit: usize) -> Verdict {
    if streak.calls <= limit {
        return Verdict::Allow;
    }
    let subject = if streak.subject.is_empty() { "this turn's question" } else { streak.subject.as_str() };
    let read: Vec<String> = streak.shapes.iter().map(|(shape, calls)| format!("`{shape}` x{calls}")).collect();
    Verdict::Deny(format!(
        "STOPPED ({UNIT}): this proxy turn made {} read-only calls on \"{subject}\" without a launch, a send or a gate, over the limit of {limit} ({LIMIT} in harness/defaults.toml). The proxy routes and judges, so this investigation is a worker's: write the brief with `north work brief new --category debugging-unknown-cause`, its `Item:` line naming \"{subject}\" and what these calls read ({}), staff it with `north fleet launch`, and judge the report.",
        streak.calls,
        read.join(", "),
    ))
}

fn tail(path: &str) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let whole = if start == 0 { &text[..] } else { text.split_once('\n').map_or("", |(_, rest)| rest) };
    Some(whole.to_owned())
}
