use crate::Shape;
use toml_edit::{DocumentMut, Item};

const DEFAULT_TYPED: &str = "one typed state enum with a single writer";

/// The `[implicit_state]` table of a structure.toml: `owners`, the path prefixes of the typed lifecycle that
/// may keep state its own way, `typed`, the pattern a finding's fix names, and `[[implicit_state.allow]]`
/// entries, each a `shape`, `path`, optional `item` and a `reason`.
#[derive(Debug)]
pub struct Config {
    pub owners: Vec<String>,
    pub typed: String,
    pub allow: Vec<Allow>,
}

#[derive(Debug)]
pub struct Allow {
    pub shape: Shape,
    pub path: String,
    pub item: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { owners: Vec::new(), typed: DEFAULT_TYPED.to_owned(), allow: Vec::new() }
    }
}

impl Config {
    pub fn parse(structure: &str) -> Result<Self, String> {
        let document = structure.parse::<DocumentMut>().map_err(|error| format!("structure.toml: {error}"))?;
        let Some(table) = document.get("implicit_state") else {
            return Ok(Self::default());
        };
        let owners = table.get("owners").and_then(Item::as_array).map(|array| array.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        let typed = table.get("typed").and_then(Item::as_str).unwrap_or(DEFAULT_TYPED).to_owned();
        let mut allow = Vec::new();
        for entry in table.get("allow").and_then(Item::as_array_of_tables).into_iter().flatten() {
            let field = |key: &str| entry.get(key).and_then(Item::as_str).map(str::to_owned);
            let (Some(shape), Some(path), Some(reason)) = (field("shape"), field("path"), field("reason")) else {
                return Err("structure.toml: each [[implicit_state.allow]] needs shape, path and reason".to_owned());
            };
            let Some(shape) = Shape::named(&shape).filter(|_| !reason.trim().is_empty()) else {
                return Err(format!("structure.toml: the [[implicit_state.allow]] entry for {path} needs a known shape and a reason"));
            };
            allow.push(Allow { shape, path, item: field("item") });
        }
        Ok(Self { owners, typed, allow })
    }

    pub(crate) fn owns(&self, path: &str) -> bool {
        self.owners.iter().any(|owner| path == owner || (owner.ends_with('/') && path.starts_with(owner.as_str())))
    }
}

impl Allow {
    pub(crate) fn covers(&self, shape: Shape, path: &str, item: &str) -> bool {
        self.shape == shape && self.path == path && self.item.as_deref().is_none_or(|wanted| wanted == item)
    }
}
