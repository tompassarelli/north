use regex::Regex;
use std::sync::LazyLock;

static CONVERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\.(?:as_str|as_deref|as_ref|trim|to_lowercase|to_string|to_owned|clone|unwrap_or_default)\(\)|\?)$").expect("conversion pattern"));
static INDEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\[\s*"([A-Za-z_]+)"\s*\]$"#).expect("index pattern"));
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[.:])(?:r#)?([A-Za-z_][A-Za-z0-9_]*)$").expect("name pattern"));
static STATUS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[a-z0-9_]*_)?(?:status|state|phase|stage|lifecycle|outcome|conclusion|verdict)$").expect("status pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^(?:Some\(\s*)?"[A-Za-z][A-Za-z0-9_ -]+"(?:\s*\))?$"#).expect("word pattern"));
static COLUMN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b([a-z_]*(?:status|state|phase|stage))\s+text\b([^,)]*)").expect("column pattern"));
static COMPARED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b([a-z_]*\.)?([a-z_]*(?:status|state|phase|stage))\s*(?:=|!=|<>)\s*'[a-z][a-z0-9_ -]*'").expect("compared pattern"));
static SQL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(create\s+table|select|update|delete\s+from|insert\s+into)\b").expect("sql pattern"));

/// The status-like name an expression ends in, after the conversions that only change its string type.
pub(crate) fn status_name(expression: &str) -> Option<String> {
    let mut rest = expression.trim();
    while let Some(found) = CONVERSION.find(rest) {
        rest = rest[..found.start()].trim_end();
    }
    let name = INDEX.captures(rest).or_else(|| NAME.captures(rest)).map(|found| found[1].to_owned())?;
    STATUS.is_match(&name).then_some(name)
}

pub(crate) fn is_word(expression: &str) -> bool {
    WORD.is_match(expression.trim())
}

/// A comparison of a status-like expression with a word literal, either way round.
pub(crate) fn compared(left: &str, right: &str) -> Option<String> {
    [(left, right), (right, left)].into_iter().find(|(name, word)| is_word(word) && status_name(name).is_some()).map(|(name, word)| format!("{} == {}", name.trim(), word.trim()))
}

/// `matches!(EXPRESSION, "a" | "b")`: the expression when it names a status and a pattern is a word literal.
pub(crate) fn matches_macro(text: &str) -> Option<String> {
    let inner = text.strip_prefix("matches!")?.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut depth = 0i32;
    let comma = inner.char_indices().find(|(_, at)| {
        match at {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        *at == ',' && depth == 0
    })?;
    let (expression, patterns) = (&inner[..comma.0], &inner[comma.0 + 1..]);
    let worded = patterns.split('|').any(|pattern| is_word(pattern.split(" if ").next().unwrap_or(pattern)));
    (worded && status_name(expression).is_some()).then(|| expression.trim().to_owned())
}

/// Each status column created as free TEXT with no CHECK naming its values, and each status compared with a
/// word in SQL, as (byte offset in the literal, item).
pub(crate) fn sql(literal: &str) -> Vec<(usize, String)> {
    if !SQL.is_match(literal) {
        return Vec::new();
    }
    let lowered = literal.to_lowercase();
    let checked = |column: &str| lowered.contains(&format!("check ({}", column.to_lowercase())) || lowered.contains(&format!("check({}", column.to_lowercase()));
    let columns = COLUMN.captures_iter(literal).filter(|found| !found[2].to_lowercase().contains("check") && !checked(&found[1])).map(|found| (found.get(0).map_or(0, |at| at.start()), format!("column {} TEXT with no CHECK", &found[1])));
    let compares = COMPARED.captures_iter(literal).map(|found| (found.get(0).map_or(0, |at| at.start()), found[0].split_whitespace().collect::<Vec<_>>().join(" ")));
    columns.chain(compares).collect()
}
