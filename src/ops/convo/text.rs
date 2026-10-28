use std::collections::BTreeSet;

pub(super) const TEXT_CAP: usize = 32 << 10;

pub(super) fn is_space(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}

pub(super) fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub(super) fn clip(text: &str) -> String {
    strip(text).chars().take(TEXT_CAP).collect()
}

pub(super) fn short_lower(text: &str, width: usize) -> String {
    text.chars().take(width).collect::<String>().to_lowercase()
}

pub(super) fn words(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|part| !part.is_empty()).collect()
}

pub(super) fn norm(text: &str) -> String {
    words(text).join(" ").to_lowercase()
}

pub(super) fn char_prefix(text: &str, width: usize) -> &str {
    text.char_indices().nth(width).map_or(text, |(index, _)| &text[..index])
}

pub(super) fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub(super) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if !matches!(character, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        lines.push(&text[start..index]);
        start = index + character.len_utf8();
        if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
            characters.next();
            start += 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

pub(super) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&if u32::from(c) < 256 { format!("\\x{:02x}", u32::from(c)) } else { format!("\\u{:04x}", u32::from(c)) }),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub(super) fn stem(term: &str) -> String {
    term.chars().take(term.chars().count().saturating_sub(2).max(4)).collect()
}

pub(super) fn stems<'a>(terms: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    terms.into_iter().map(stem).collect()
}

pub(super) fn short_project(project: Option<&str>) -> String {
    let text = project.unwrap_or("");
    for prefix in ["-home-tom-code-client-", "-home-tom-code-", "-home-tom-"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return rest.to_owned();
        }
    }
    if text == "-home-tom" {
        return "home".to_owned();
    }
    let trimmed = text.trim_start_matches('-');
    if trimmed.is_empty() { "?".to_owned() } else { trimmed.to_owned() }
}
