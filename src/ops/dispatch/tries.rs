//! Whether a dispatched session spent a try on its item: a commit, a check run or a report other than
//! a capacity wait spends one; a session that only waited on admission, a usage limit or a failed host
//! did no work, so dispatch does not count it toward the two tries that hand an item to the proxy.
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b[^\n|;&]*\bcommit\b|\bsafe-push\b").expect("commit pattern"));
static CHECK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bcargo\s+(?:test|clippy|nextest)\b|\bbun\s+(?:test|run\s+check)\b|\s--check\b|\.test\.sh\b|\bverify-north\b").expect("check pattern"));
static CAPACITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bDEFER_[A-Z_]+|\b(?:wait(?:s|ed|ing)?|queued|held)\b[^.\n]{0,60}\b(?:pairs?|pool|queue|admission|admitted|slots?|GPU|lease)\b|\bin the (?:pool )?queue\b|\busage limit\b|\bhit your limit\b|\brate.?limit|\bhost (?:failure|is down|unreachable)\b|\bconnection (?:refused|reset|timed out)\b",
    )
    .expect("capacity pattern")
});

/// Why the session in `north fleet show NAME --json` spent no try, or None when it did work.
pub(crate) fn unspent(session: &Value) -> Option<String> {
    let entries = session["entries"].as_array()?;
    let text = |kind: &'static str| entries.iter().filter(move |entry| entry["kind"] == kind).filter_map(|entry| entry["text"].as_str());
    if text("tool").any(|tool| COMMIT.is_match(tool) || CHECK.is_match(tool)) {
        return None;
    }
    let report = text("assistant").next_back().or_else(|| entries.last().and_then(|entry| entry["text"].as_str())).unwrap_or_default().trim();
    if let Some(found) = CAPACITY.find(report) {
        let start = report[..found.start()].rfind(['.', '\n']).map_or(0, |end| end + 1);
        let end = report[found.end()..].find(['.', '\n']).map_or(report.len(), |end| found.end() + end);
        return Some(format!("only waited on capacity: {}", report[start..end].trim()));
    }
    let exit = session["turns"].as_array()?.last()?["exit"].as_str()?;
    (text("assistant").next().is_none() && exit != "completed").then(|| format!("{exit} before any report, a host failure"))
}
