use regex::Regex;
use std::sync::LazyLock;

const SPACE: &str = r"[\s\x1c-\x1f]";
const PLACEHOLDER_WORDS: [&str; 21] = [
    "ambient", "inherit", "inherited", "lineage", "self", "parent", "default", "auto", "current",
    "unobserved", "selected", "selection", "available", "best", "highest", "lowest", "fallback",
    "unspecified", "unknown", "pending", "selector",
];
const ADMITTED: [&str; 15] = [
    "claude-fable-5",
    "claude-fable-5-1",
    "claude-haiku-5-5",
    "claude-opus-4-8",
    "claude-opus-5",
    "claude-opus-5-5",
    "claude-sonnet-5",
    "gpt-5",
    "gpt-6-astra",
    "gpt-6-luna",
    "gpt-6-sol",
    "gpt-6.1-sol",
    "gpt-5.6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
];
const MODEL_TABLE_HEADERS: [&str; 6] = [
    "actor",
    "actor / model",
    "account / model",
    "model",
    "model / actor",
    "workers",
];
pub const CALIBRATION: &str = "estimate-calibration.md";
pub const LEDGER: &str = "model-assignment-ledger.md";

fn placeholders() -> String {
    PLACEHOLDER_WORDS.join("|")
}

fn regex(pattern: String) -> Regex {
    Regex::new(&pattern).unwrap()
}

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!("(?i)(?:^|[^a-z0-9])(?:{})(?:[^a-z0-9]|$)", placeholders()))
});
static ADMITTED_MODEL: LazyLock<Regex> = LazyLock::new(|| {
    let mut identities = ADMITTED.to_vec();
    identities.sort_by_key(|identity| std::cmp::Reverse(identity.len()));
    let alternation: Vec<String> = identities.iter().map(|i| regex::escape(i)).collect();
    regex(format!("(?:^|[^a-z0-9])(?:{})(?:[^a-z0-9]|$)", alternation.join("|")))
});
static FIELD: LazyLock<Regex> =
    LazyLock::new(|| regex(format!(r"(?i)^{SPACE}*(?:reviewer_)?model{SPACE}*={SPACE}*(.+)$")));
static CALIBRATION_COLON: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!(r"(?i)(?:^|[;|—]){SPACE}*((?:reviewer )?model){SPACE}*:{SPACE}*([^;|]+)"))
});
static CALIBRATION_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!(r"(?i)(?:^|[;|—]){SPACE}*((?:reviewer_)?model){SPACE}*={SPACE}*([^\s\x1c-\x1f;|]+)"))
});
static CALIBRATION_PROSE: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!(
        r"(?im)^[^|\n]*—{SPACE}+(?:(?:one|two|three|four){SPACE}+)?(?:{})(?:[- ](?:parent|root|model))?(?:{SPACE}|$)",
        placeholders()
    ))
});
static TABLE_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| regex(r"^:?-{3,}:?\z".into()));
static COMMAND_MODEL: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!(
        r#"(?i)(?:reviewer_)?model{SPACE}*={SPACE}*(?:"([^"]*)"|'([^']*)'|([a-z0-9._:/-]+))"#
    ))
});
static CALIBRATION_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    regex(format!(
        r#"(?i)(?:^|[;'"|—]){SPACE}*(?:model|reviewer model){SPACE}*:(?:{SPACE}*|{SPACE}*[^;\n|]*?[^a-z0-9;\n|])(?:{})(?:[^a-z0-9]|$)"#,
        placeholders()
    ))
});

pub fn is_space(c: char) -> bool {
    crate::shell::whitespace(c)
}

pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !matches!(
            c,
            '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            continue;
        }
        lines.push(&text[start..index]);
        start = index + c.len_utf8();
        if c == '\r'
            && let Some(&(next, '\n')) = chars.peek()
        {
            chars.next();
            start = next + 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn first_model_token(value: &str) -> &str {
    trim(value)
        .split(is_space)
        .next()
        .unwrap_or("")
        .trim_matches(['`', '\'', '"', ',', ';'])
}

pub fn concrete(value: &str) -> bool {
    ADMITTED.contains(&value)
}

fn contains_concrete(value: &str) -> bool {
    ADMITTED_MODEL.is_match(value)
}

fn table_columns(line: &str) -> Option<Vec<&str>> {
    let stripped = trim(line);
    if !(stripped.starts_with('|') && stripped.ends_with('|')) {
        return None;
    }
    let inner = if stripped.len() < 2 { "" } else { &stripped[1..stripped.len() - 1] };
    Some(inner.split('|').map(trim).collect())
}

fn table_separator(columns: Option<&Vec<&str>>) -> bool {
    columns.is_some_and(|columns| {
        !columns.is_empty() && columns.iter().all(|column| TABLE_SEPARATOR.is_match(column))
    })
}

fn model_column(columns: &[&str]) -> Option<usize> {
    columns.iter().position(|column| {
        let normalized = column.to_lowercase().split(is_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        MODEL_TABLE_HEADERS.contains(&normalized.as_str())
    })
}

fn calibration_table_invalid(text: &str) -> bool {
    let lines = splitlines(text);
    let mut model = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(columns) = table_columns(line) else {
            model = None;
            continue;
        };
        let next = lines.get(index + 1).and_then(|next| table_columns(next));
        if table_separator(next.as_ref()) {
            model = model_column(&columns);
            continue;
        }
        if table_separator(Some(&columns)) {
            continue;
        }
        if let Some(column) = model.and_then(|m| columns.get(m))
            && PLACEHOLDER.is_match(column)
            && !contains_concrete(column)
        {
            return true;
        }
    }
    false
}

fn calibration_line_invalid(line: &str) -> bool {
    [&*CALIBRATION_COLON, &*CALIBRATION_INLINE].iter().any(|pattern| {
        pattern.captures_iter(line).any(|found| {
            let token = first_model_token(&found[2]);
            let skipped =
                found[1].to_lowercase().starts_with("reviewer") && token.to_lowercase() == "none";
            !skipped && !concrete(token)
        })
    })
}

pub fn line_invalid(line: &str, filename: &str) -> bool {
    if filename == LEDGER && trim(line).starts_with('#') {
        return false;
    }
    if FIELD.captures(line).is_some_and(|found| !concrete(first_model_token(&found[1]))) {
        return true;
    }
    if filename == CALIBRATION && calibration_line_invalid(line) {
        return true;
    }
    if filename == LEDGER {
        let columns: Vec<&str> = line.split('|').map(trim).collect();
        if columns.len() >= 5 && !concrete(first_model_token(columns[2])) {
            return true;
        }
    }
    false
}

pub fn text_invalid(text: &str, filename: &str) -> bool {
    if filename == CALIBRATION && (CALIBRATION_PROSE.is_match(text) || calibration_table_invalid(text)) {
        return true;
    }
    splitlines(text).iter().any(|line| line_invalid(line, filename))
}

pub fn command_invalid(command: &str) -> bool {
    let mut position = 0;
    while let Some(found) = COMMAND_MODEL.captures_at(command, position) {
        let whole = found.get(0).unwrap();
        let before = command[..whole.start()].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            position = whole.start() + command[whole.start()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let value = (1..=3).find_map(|group| found.get(group)).map_or("", |m| m.as_str());
        if !concrete(value) {
            return true;
        }
        position = whole.end();
    }
    false
}

pub fn calibration_command_placeholder(command: &str) -> bool {
    CALIBRATION_COMMAND.is_match(command)
}
