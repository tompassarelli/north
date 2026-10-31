use serde_json::Value;

pub fn parse(text: &str) -> Result<Value, toml_edit::TomlError> {
    Ok(value(text.parse::<toml_edit::DocumentMut>()?.as_item()))
}

pub fn value(item: &toml_edit::Item) -> Value {
    if let Some(table) = item.as_table_like() {
        return Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.to_owned(), value(v)))
                .collect(),
        );
    }
    if let Some(array) = item.as_array_of_tables() {
        return Value::Array(
            array
                .iter()
                .map(|t| value(&toml_edit::Item::Table(t.clone())))
                .collect(),
        );
    }
    match item.as_value() {
        Some(toml_edit::Value::String(v)) => v.value().clone().into(),
        Some(toml_edit::Value::Integer(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Float(v)) => {
            serde_json::Number::from_f64(*v.value()).map_or(Value::Null, Value::Number)
        }
        Some(toml_edit::Value::Boolean(v)) => (*v.value()).into(),
        Some(toml_edit::Value::Array(a)) => Value::Array(
            a.iter()
                .map(|v| value(&toml_edit::Item::Value(v.clone())))
                .collect(),
        ),
        _ => Value::Null,
    }
}
