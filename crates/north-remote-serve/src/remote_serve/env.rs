use super::Refusal;

pub(super) fn strip_prefix(words: &[String]) -> Result<&[String], Refusal> {
    match words.split_first() {
        Some((first, rest)) if first == "env" => {
            if let Some(word) = rest.iter().find(|word| north_base::shell::assignment(word)) {
                let (key, _) = word.split_once('=').unwrap_or((word, ""));
                return Err(Refusal::EnvKey(key.to_string()));
            }
            Ok(words)
        }
        _ => Ok(words),
    }
}
