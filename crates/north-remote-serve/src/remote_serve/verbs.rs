use super::Refusal;

pub(super) fn authorize(argv: &[String]) -> Result<String, Refusal> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let refused = || Refusal::Verb(words.iter().take(3).copied().collect::<Vec<_>>().join(" "));
    let verb = match words.as_slice() {
        ["readlink", ".local/state/north/current"] => Some("readlink current".to_string()),
        ["north", "runtime", "health"] => Some("runtime health".to_string()),
        ["north", "holds", "list", "--here", "--json"] => Some("holds list".to_string()),
        ["south", "land"] => Some("south land".to_string()),
        _ => None,
    };
    verb.ok_or_else(refused)
}
