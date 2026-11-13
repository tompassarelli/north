use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMode {
    #[default]
    North,
    Vanilla,
}

impl PromptMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "north" => Some(PromptMode::North),
            "vanilla" => Some(PromptMode::Vanilla),
            _ => None,
        }
    }
}
