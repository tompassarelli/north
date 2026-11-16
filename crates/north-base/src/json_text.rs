pub fn ascii_string(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("a string encodes");
    let mut out = String::with_capacity(encoded.len());
    for character in encoded.chars() {
        if character.is_ascii() {
            out.push(character);
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

pub fn ascii_string_escaping_del(text: &str) -> String {
    ascii_string(text).replace('\u{7f}', "\\u007f")
}

pub fn toml_value(item: &toml_edit::Item) -> serde_json::Value {
    use serde_json::Value;
    use toml_edit::Item;
    if let Some(table) = item.as_table_like() {
        return Value::Object(table.iter().map(|(k, v)| (k.to_owned(), toml_value(v))).collect());
    }
    if let Some(array) = item.as_array_of_tables() {
        return Value::Array(array.iter().map(|t| toml_value(&Item::Table(t.clone()))).collect());
    }
    match item.as_value() {
        Some(toml_edit::Value::String(v)) => v.value().clone().into(),
        Some(toml_edit::Value::Integer(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Float(v)) => serde_json::Number::from_f64(*v.value()).map_or(Value::Null, Value::Number),
        Some(toml_edit::Value::Boolean(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Datetime(v)) => v.value().to_string().into(),
        Some(toml_edit::Value::Array(a)) => Value::Array(a.iter().map(|v| toml_value(&Item::Value(v.clone()))).collect()),
        _ => Value::Null,
    }
}
