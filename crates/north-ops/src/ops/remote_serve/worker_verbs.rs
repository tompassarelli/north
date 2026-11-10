use super::Refusal;

const FLEET_SPOOL: [&str; 2] = ["cache", "registry"];

pub(super) fn authorize(argv: &[String]) -> Result<String, Refusal> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let verb = match words.as_slice() {
        ["north", "fleet", "spool", which] if FLEET_SPOOL.contains(which) => Some(format!("fleet spool {which}")),
        ["north", "fleet", "list", "--json"] => Some("fleet list".to_string()),
        ["north", "threads", "list", "--json"] => Some("threads list".to_string()),
        ["north", "dash", "--closed", "--since", span] if span_word(span) => Some("dash closed".to_string()),
        ["north", "runtime", "health"] => Some("runtime health".to_string()),
        ["readlink", ".local/state/north/current"] => Some("readlink current".to_string()),
        _ => None,
    };
    verb.ok_or_else(|| Refusal::Verb(words.iter().take(3).copied().collect::<Vec<_>>().join(" ")))
}

fn span_word(span: &str) -> bool {
    let digits = span.trim_end_matches(['h', 'd']);
    span.len() == digits.len() + 1 && !digits.is_empty() && digits.len() <= 4 && digits.bytes().all(|b| b.is_ascii_digit())
}
