use rusqlite::types::Value;

use super::store::Row;

pub(super) fn object(row: Row, stale: bool) -> String {
    let fields: Vec<String> = row.values().map(|(name, value)| format!("\"{name}\": {}", value_json(value))).collect();
    format!("{{{}, \"stale\": {stale}}}", fields.join(", "))
}

fn value_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Integer(number) => number.to_string(),
        Value::Real(number) => real(*number),
        Value::Text(text) => string(text),
        Value::Blob(_) => "null".into(),
    }
}

fn real(number: f64) -> String {
    if number.is_nan() {
        return "NaN".into();
    }
    if number.is_infinite() {
        return if number > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let text = format!("{number:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            let (sign, digits) = exponent.strip_prefix('-').map_or(("+", exponent), |digits| ("-", digits));
            format!("{mantissa}e{sign}{digits:0>2}")
        }
        None => text,
    }
}

fn string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(character),
            _ => {
                let mut units = [0u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}
