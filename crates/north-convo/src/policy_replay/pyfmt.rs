pub(super) fn is_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\x1c'..='\x1f')
}

pub(super) fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub(super) fn lstrip(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c'..='\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

pub(super) fn splitlines(text: &str, keepends: bool) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if !is_line_break(c) {
            continue;
        }
        let mut end = at + c.len_utf8();
        if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
            chars.next();
            end += 1;
        }
        lines.push(&text[start..if keepends { end } else { at }]);
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

pub(super) fn round_to(value: f64, places: usize) -> f64 {
    format!("{value:.places$}").parse().unwrap_or(value)
}

pub(super) fn round_int(value: f64) -> i64 {
    value.round_ties_even() as i64
}

pub(super) fn float_repr(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() { "nan".into() } else if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |rest| ("-", rest));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if !(-4..16).contains(&exponent) {
        let (head, tail) = digits.split_at(1);
        let fraction = if tail.is_empty() { String::new() } else { format!(".{tail}") };
        return format!("{sign}{head}{fraction}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    if exponent < 0 {
        return format!("{sign}0.{}{digits}", "0".repeat(exponent.unsigned_abs() as usize - 1));
    }
    let whole = exponent as usize + 1;
    let padded = format!("{digits:0<whole$}");
    let (integer, fraction) = padded.split_at(whole);
    format!("{sign}{integer}.{}", if fraction.is_empty() { "0" } else { fraction })
}

pub(super) fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            ' '..='~' | '\x7f' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

pub(super) fn str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub(super) fn list_repr(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|item| str_repr(item)).collect::<Vec<_>>().join(", "))
}
