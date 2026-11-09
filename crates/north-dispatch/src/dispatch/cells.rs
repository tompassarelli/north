use north_base::platform::harness;

pub const TABLE: &str = "harness/routing-cells.toml";
const EFFORT_WORDS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cell {
    pub model: &'static str,
    pub effort: &'static str,
}

impl Cell {
    pub fn name(self) -> String {
        format!("{}-{}", self.model, self.effort)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    pub model: Option<&'static str>,
    pub effort: Option<&'static str>,
    pub categories: Vec<&'static str>,
    pub reason: &'static str,
}

impl Exclusion {
    fn matches(&self, category: &str, model: &str, effort: &str) -> bool {
        self.model.is_none_or(|named| named == model)
            && self.effort.is_none_or(|named| named == effort)
            && (self.categories.is_empty() || self.categories.contains(&category))
    }
}

fn strings(key: &str) -> Vec<&'static str> {
    harness::document(TABLE).get(key).and_then(toml_edit::Item::as_array).map(|array| array.iter().filter_map(toml_edit::Value::as_str).collect()).unwrap_or_default()
}

pub fn models() -> Vec<&'static str> {
    strings("models")
}

pub fn efforts() -> Vec<&'static str> {
    strings("efforts")
}

pub fn exclusions() -> Vec<Exclusion> {
    let document = harness::document(TABLE);
    let Some(tables) = document.get("exclude").and_then(toml_edit::Item::as_array_of_tables) else { return Vec::new() };
    tables
        .iter()
        .map(|table| Exclusion {
            model: table.get("model").and_then(toml_edit::Item::as_str),
            effort: table.get("effort").and_then(toml_edit::Item::as_str),
            categories: table.get("categories").and_then(toml_edit::Item::as_array).map(|array| array.iter().filter_map(toml_edit::Value::as_str).collect()).unwrap_or_default(),
            reason: table.get("reason").and_then(toml_edit::Item::as_str).unwrap_or_default(),
        })
        .collect()
}

pub fn allowed(category: &str) -> Vec<Cell> {
    let exclusions = exclusions();
    models()
        .into_iter()
        .flat_map(|model| efforts().into_iter().map(move |effort| Cell { model, effort }))
        .filter(|cell| !exclusions.iter().any(|row| row.matches(category, cell.model, cell.effort)))
        .collect()
}

pub fn excluded(category: &str, model: &str, effort: &str) -> Option<&'static str> {
    let model = family(model).unwrap_or(model);
    exclusions().into_iter().find(|row| row.matches(category, model, effort)).map(|row| row.reason)
}

pub fn family(model: &str) -> Option<&'static str> {
    let model = model.to_lowercase();
    models().into_iter().find(|family| model.contains(family))
}

pub fn table_line() -> String {
    let cells: Vec<String> = allowed("").into_iter().map(Cell::name).collect();
    format!("arm cells ({}): {}", TABLE, cells.join(", "))
}

pub fn named(model: &str, effort: &str) -> Option<String> {
    let model = family(model)?;
    let effort = EFFORT_WORDS.into_iter().find(|word| *word == effort)?;
    Some(format!("{model}-{effort}"))
}
