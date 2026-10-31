use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row(Vec<(String, Value)>);

impl Row {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(key, value);
        self
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        let (key, value) = (key.into(), value.into());
        match self.0.iter_mut().find(|(known, _)| *known == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn extended(mut self, other: Row) -> Self {
        for (key, value) in other.0 {
            self.set(key, value);
        }
        self
    }

    pub fn get(&self, key: &str) -> &Value {
        self.0.iter().find(|(known, _)| known == key).map_or(&Value::Null, |(_, value)| value)
    }

    pub fn to_json(&self) -> String {
        self.render(|value| value.clone())
    }

    pub fn to_rounded_json(&self) -> String {
        self.render(|value| match value.as_f64() {
            Some(float) if value.is_f64() => Value::from((float * 1e4).round_ties_even() / 1e4),
            _ => value.clone(),
        })
    }

    fn render(&self, shown: impl Fn(&Value) -> Value) -> String {
        let fields: Vec<String> = self.0.iter().map(|(key, value)| format!("{}: {}", Value::from(key.as_str()), spaced(&shown(value)))).collect();
        format!("{{{}}}", fields.join(", "))
    }
}

fn spaced(value: &Value) -> String {
    match value {
        Value::Array(items) => format!("[{}]", items.iter().map(spaced).collect::<Vec<_>>().join(", ")),
        Value::Object(fields) => format!("{{{}}}", fields.iter().map(|(key, value)| format!("{}: {}", Value::from(key.as_str()), spaced(value))).collect::<Vec<_>>().join(", ")),
        scalar => scalar.to_string(),
    }
}
