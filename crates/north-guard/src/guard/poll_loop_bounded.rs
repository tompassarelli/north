use std::sync::LazyLock;

use regex::Regex;

static WRAPPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\btimeout\s+(?:-\S+\s+)*[0-9.]+[smhd]?(\s+(?:ba)?sh\s+-c\s)").unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s;&|(!'\x22])(until|while|for|done)(?:[\s;&|)]|$)").unwrap());
static SETS_STATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s;&|(])(?:[A-Za-z_][A-Za-z0-9_]*=|[A-Za-z_][A-Za-z0-9_]*\s*\(\)|function\s)").unwrap());

pub(super) fn command(source: &str, seconds: f64) -> Option<String> {
    if WRAPPED.is_match(source) {
        return Some(WRAPPED.replace(source, format!("timeout {seconds:.0}$1")).into_owned());
    }
    let mut depth = 0usize;
    let mut start = None;
    for found in WORD.captures_iter(source) {
        let word = found.get(1)?;
        if word.as_str() == "done" {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                let start = start?;
                return (!SETS_STATE.is_match(&source[..start])).then(|| wrap(source, start, word.end(), seconds));
            }
        } else {
            start.get_or_insert(word.start());
            depth += 1;
        }
    }
    None
}

fn wrap(source: &str, start: usize, end: usize, seconds: f64) -> String {
    let looped = source[start..end].replace('\'', r"'\''");
    let (before, after) = (&source[..start], &source[end..]);
    let after = match after.trim_start().strip_prefix(';') {
        Some(rest) => format!(" &&{rest}"),
        None => after.to_owned(),
    };
    format!("{before}timeout {seconds:.0} bash -c '{looped}'{after}")
}
