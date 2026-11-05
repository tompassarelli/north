use std::collections::BTreeMap;

use super::Refusal;

const ACTOR_LENGTH: std::ops::RangeInclusive<usize> = 1..=64;
const OWNERS_BYTES: usize = 64 * 1024;
const GRACE_DIGITS: std::ops::RangeInclusive<usize> = 1..=6;

pub(super) fn split_prefix(words: &[String]) -> Result<(BTreeMap<String, String>, &[String]), Refusal> {
    let mut env = BTreeMap::new();
    let rest = match words.split_first() {
        Some((first, rest)) if first == "env" => {
            let assignments = rest.iter().take_while(|word| north_base::shell::assignment(word)).count();
            for word in &rest[..assignments] {
                let (key, value) = word.split_once('=').unwrap_or((word, ""));
                if let Some(kept) = filter(key, value)?
                    && env.insert(key.to_string(), kept).is_some()
                {
                    return Err(Refusal::EnvRepeated(key.to_string()));
                }
            }
            &rest[assignments..]
        }
        _ => words,
    };
    env.insert("THREADS_NO_NATIVE".to_string(), "1".to_string());
    Ok((env, rest))
}

fn filter(key: &str, value: &str) -> Result<Option<String>, Refusal> {
    let accepted = match key {
        "THREADS_ORIGIN" | "THREADS_NO_NATIVE" => return Ok(None),
        "THREADS_ACTOR" | "LAPTOP_BY" => actor(value),
        "THREADS_ALIVE_OWNERS" => owners(value),
        "THREADS_ARCHIVES_LOCAL" => value == "1",
        "LAPTOP_GRACE" => GRACE_DIGITS.contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit()),
        _ => return Err(Refusal::EnvKey(key.to_string())),
    };
    if accepted {
        Ok(Some(value.to_string()))
    } else {
        Err(Refusal::EnvValue(key.to_string()))
    }
}

fn actor(value: &str) -> bool {
    ACTOR_LENGTH.contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@:-".contains(&b))
}

fn owners(value: &str) -> bool {
    value.len() <= OWNERS_BYTES
        && serde_json::from_str::<Vec<String>>(value).is_ok_and(|names| names.iter().all(|name| actor(name)))
}
