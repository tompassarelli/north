use super::Refusal;

pub(super) fn authorize(argv: &[String]) -> Result<String, Refusal> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let verb = match words.as_slice() {
        ["north", "runtime", "health"] => Some("runtime health".to_string()),
        ["readlink", ".local/state/north/current"] => Some("readlink current".to_string()),
        _ => None,
    };
    verb.ok_or_else(|| Refusal::Verb(words.iter().take(3).copied().collect::<Vec<_>>().join(" ")))
}
