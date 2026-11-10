use serde_json::Value;

use crate::policy::name_ok;

const FIELDS: [&str; 2] = ["name", "private"];

pub fn private_repository(body: &[u8]) -> Result<(), String> {
    let Value::Object(fields) = serde_json::from_slice(body).map_err(|error| format!("the body is not JSON: {error}"))? else {
        return Err("the body is not a JSON object".to_owned());
    };
    if let Some(extra) = fields.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(format!("the body carries {extra}; the broker creates a repository from its name and private alone"));
    }
    if fields.get("private") != Some(&Value::Bool(true)) {
        return Err("the body must set private to true; the broker creates only private repositories".to_owned());
    }
    match fields.get("name").and_then(Value::as_str) {
        Some(name) if name_ok(name) && !matches!(name, "." | "..") => Ok(()),
        _ => Err("the body needs a repository name of letters, digits, '-', '_' or '.'".to_owned()),
    }
}
