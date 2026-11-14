use toml_edit::{Item, TableLike, Value};

use super::schemas::{Schema, Ty};
use Ty::{Any, Day, Flag, List, Money, Nested, Number, Text};

fn matches(ty: Ty, item: &Item) -> bool {
    let value = item.as_value();
    match ty {
        Text => value.is_some_and(Value::is_str),
        Number => value.is_some_and(|value| value.is_integer() || value.is_float()),
        Money => value.is_some_and(|value| value.is_integer() || value.is_float() || value.is_str()),
        Day => value.is_some_and(|value| value.is_datetime() || value.is_str()),
        Flag => value.is_some_and(Value::is_bool),
        List => value.is_some_and(Value::is_array),
        Nested | Any => true,
    }
}

fn nodes(item: &Item) -> Vec<(String, &dyn TableLike)> {
    if let Some(array) = item.as_array_of_tables() {
        return array.iter().enumerate().map(|(index, table)| (format!("[{index}]"), table as &dyn TableLike)).collect();
    }
    if let Some(array) = item.as_value().and_then(Value::as_array) {
        return array.iter().enumerate().filter_map(|(index, value)| Some((format!("[{index}]"), value.as_inline_table()? as &dyn TableLike))).collect();
    }
    item.as_table_like().map(|table| vec![(String::new(), table)]).unwrap_or_default()
}

pub(super) fn walk(schema: &Schema, path: &str, label: &str, node: &dyn TableLike, found: &mut Vec<String>) {
    let Some(table) = schema.tables.iter().find(|table| table.path == path) else { return };
    let at = |key: &str| if label.is_empty() { key.to_owned() } else { format!("{label}.{key}") };
    for key in table.required.iter().filter(|key| node.get(key).is_none()) {
        found.push(format!("{}: {}: required key is missing", schema.file, at(key)));
    }
    for (key, item) in node.iter() {
        let Some((name, ty)) = table.keys.iter().find(|(name, _)| *name == key).or_else(|| table.keys.iter().find(|(name, _)| *name == "*")) else {
            found.push(format!("{}: {}: unknown key", schema.file, at(key)));
            continue;
        };
        if !matches(*ty, item) {
            found.push(format!("{}: {}: wrong type of value", schema.file, at(key)));
        } else if matches!(ty, Nested) {
            let child = if path.is_empty() { (*name).to_owned() } else { format!("{path}.{name}") };
            for (index, inner) in nodes(item) {
                walk(schema, &child, &format!("{}{index}", at(key)), inner, found);
            }
        }
    }
}

