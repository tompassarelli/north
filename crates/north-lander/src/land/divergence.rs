pub const TRAILER: &str = "Replay-change:";
const KINDS: [&str; 8] = ["landed", "failed", "conflict", "undecided", "deferred", "transient", "red-main", "error"];

pub fn declared_kind(line: &str) -> Option<Result<(String, String), String>> {
    let rest = line.strip_prefix(TRAILER)?.trim();
    let (name, reason) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(name, reason)| (name, reason.trim()));
    let kind = name.strip_prefix("land.").filter(|kind| KINDS.contains(kind));
    Some(match kind {
        Some(kind) if !reason.is_empty() => Ok((kind.to_owned(), reason.to_owned())),
        Some(_) => Err(format!("'{line}' gives no reason after land.<kind>")),
        None => Err(format!("'{line}' names no replay decision; write {TRAILER} land.<kind> <reason>, kind one of {}", KINDS.join(", "))),
    })
}

pub fn refuse_malformed(bodies: &str) -> Result<(), String> {
    match bodies.lines().filter_map(declared_kind).find_map(Result::err) {
        Some(error) => Err(format!("{error}; NOT queued")),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "divergence_tests.rs"]
mod tests;
