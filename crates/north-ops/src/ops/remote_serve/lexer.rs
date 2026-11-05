use super::Refusal;

const BARE_PUNCTUATION: &str = "._-/@:=,+%";

pub(super) fn split(command: &str) -> Result<Vec<String>, Refusal> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) if inner.is_control() => return Err(Refusal::Metacharacter(inner)),
                        Some(inner) => word.push(inner),
                        None => return Err(Refusal::UnterminatedQuote),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some(inner @ ('$' | '`' | '\\' | '!')) => return Err(Refusal::Metacharacter(inner)),
                        Some(inner) if inner.is_control() => return Err(Refusal::Metacharacter(inner)),
                        Some(inner) => word.push(inner),
                        None => return Err(Refusal::UnterminatedQuote),
                    }
                }
            }
            '#' if started => {
                word.push(c);
            }
            c if c.is_ascii_alphanumeric() || BARE_PUNCTUATION.contains(c) => {
                started = true;
                word.push(c);
            }
            c => return Err(Refusal::Metacharacter(c)),
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}
