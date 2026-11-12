use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub parts: Vec<Part>,
    pub shared: Vec<Declaration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub summary: String,
    pub files: Vec<Edit>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    pub path: String,
    pub new: bool,
    pub adds: Vec<Symbol>,
    pub changes: Vec<Symbol>,
    pub uses: Vec<Symbol>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub symbol: Symbol,
    pub path: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Symbol {
    pub owner: Option<String>,
    pub name: String,
}

impl Symbol {
    pub fn parse(text: &str) -> Option<Self> {
        let path = text.split(['(', '<', ' ']).next().unwrap_or_default().trim_end_matches('!');
        let mut segments = path.rsplit("::").filter(|segment| !segment.is_empty() && *segment != "crate" && *segment != "self" && *segment != "super");
        let name = segments.next()?.to_owned();
        name.chars().all(|c| c.is_alphanumeric() || c == '_').then(|| Self { owner: segments.next().map(str::to_owned), name })
    }

    pub fn shown(&self) -> String {
        self.owner.as_ref().map_or_else(|| self.name.clone(), |owner| format!("{owner}::{}", self.name))
    }
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

fn symbols(value: &Value, key: &str) -> Vec<Symbol> {
    value.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(Symbol::parse).collect()
}

fn edit(value: &Value) -> Result<Edit, String> {
    let path = text(value, "path");
    if path.is_empty() {
        return Err("a file of the plan has no path".to_owned());
    }
    Ok(Edit { path, new: value.get("new").and_then(Value::as_bool).unwrap_or(false), adds: symbols(value, "adds"), changes: symbols(value, "changes"), uses: symbols(value, "uses") })
}

fn part(value: &Value) -> Result<Part, String> {
    let files = value.get("files").and_then(Value::as_array).ok_or("a part of the plan lists no files")?;
    Ok(Part { name: text(value, "name"), summary: text(value, "summary"), files: files.iter().map(edit).collect::<Result<_, _>>()? })
}

fn declaration(value: &Value) -> Result<Declaration, String> {
    let symbol = Symbol::parse(&text(value, "symbol")).ok_or("a shared declaration names no symbol")?;
    Ok(Declaration { symbol, path: text(value, "path"), signature: text(value, "signature") })
}

impl Plan {
    pub fn parse(answer: &str) -> Result<(Self, Value), String> {
        let (Some(start), Some(end)) = (answer.find('{'), answer.rfind('}')) else { return Err("the plan holds no JSON object".to_owned()) };
        let value: Value = serde_json::from_str(answer.get(start..=end).unwrap_or_default()).map_err(|why| format!("the plan is not JSON: {why}"))?;
        let parts = value.get("parts").and_then(Value::as_array).ok_or("the plan has no parts list")?;
        let shared = value.get("shared").and_then(Value::as_array).map(|shared| shared.iter().map(declaration).collect::<Result<_, _>>()).transpose()?.unwrap_or_default();
        Ok((Self { parts: parts.iter().map(part).collect::<Result<_, _>>()?, shared }, value))
    }

    pub fn shape() -> Value {
        json!({
            "parts": [{ "name": "short name", "summary": "one sentence", "files": [{ "path": "crates/x/src/y.rs", "new": false, "adds": ["Owner::name"], "changes": ["name"], "uses": ["Owner::name"] }] }],
            "shared": [{ "symbol": "Owner", "path": "crates/x/src/types.rs", "signature": "pub struct Owner { pub field: u32 }" }]
        })
    }
}
