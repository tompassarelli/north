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
        Value::Text(text) => north_base::json_text::ascii_string(text),
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
