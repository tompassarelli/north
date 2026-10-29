use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Number, Value};

/// A JSON object that keeps insertion order, and like a JavaScript object keeps a key's first position when it is set again.
#[derive(Clone, Debug, Default)]
pub(crate) struct Obj(Vec<(String, Value)>);

impl Obj {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn put(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.set(key, value);
        self
    }

    pub(crate) fn put_some(self, key: &str, value: Option<impl Into<Value>>) -> Self {
        match value {
            Some(value) => self.put(key, value),
            None => self,
        }
    }

    pub(crate) fn set(&mut self, key: &str, value: impl Into<Value>) {
        let value = js_numbers(value.into());
        match self.0.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }

    pub(crate) fn spread(mut self, other: &Value) -> Self {
        if let Value::Object(map) = other {
            for (key, value) in map {
                self.set(key, value.clone());
            }
        }
        self
    }

    pub(crate) fn merge(mut self, other: Obj) -> Self {
        for (key, value) in other.0 {
            self.set(&key, value);
        }
        self
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub(crate) fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub(crate) fn value(&self) -> Value {
        Value::Object(self.0.iter().cloned().collect())
    }
}

impl Serialize for Obj {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// The value with every number written as JavaScript would write it.
pub(crate) fn js_numbers(value: Value) -> Value {
    match value {
        Value::Number(number) if number.is_f64() => num(number.as_f64().unwrap_or(f64::NAN)),
        Value::Array(items) => Value::Array(items.into_iter().map(js_numbers).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(key, value)| (key, js_numbers(value))).collect()),
        other => other,
    }
}

/// A number as JavaScript's JSON.stringify writes it: whole values without a fraction, non-finite values as null.
pub(crate) fn num(value: f64) -> Value {
    if !value.is_finite() {
        Value::Null
    } else if value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        Value::Number(Number::from(value as i64))
    } else {
        Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

pub(crate) fn opt(value: Option<f64>) -> Value {
    value.map_or(Value::Null, num)
}

/// A number as JavaScript's template strings print it.
pub(crate) fn show(value: f64) -> String {
    match num(value) {
        Value::Null if value.is_nan() => "NaN".to_owned(),
        Value::Null => if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned(),
        number => number.to_string(),
    }
}

/// `Number(text)`: blank is zero and anything that is not a decimal or hex literal is NaN.
pub(crate) fn number(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() {
        return 0.0;
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |whole| whole as f64);
    }
    match text.trim_start_matches(['+', '-']) {
        "Infinity" => return if text.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY },
        rest if rest.chars().any(|character| character.is_ascii_alphabetic() && !matches!(character, 'e' | 'E')) => return f64::NAN,
        _ => {}
    }
    text.parse().unwrap_or(f64::NAN)
}

pub(crate) fn round(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale + 0.5).floor() / scale
}

pub(crate) fn round2(value: f64) -> f64 {
    round(value, 2)
}

/// A field as JavaScript reads it from a parsed object: a number, or None for anything else.
pub(crate) fn field(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
