use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn parse(text: &str) -> NorthResult<Self> {
        Self::ALL
            .into_iter()
            .find(|effort| effort.label() == text)
            .ok_or_else(|| NorthError::Usage(format!("no effort {text}; use one of {}", Self::ALL.map(Self::label).join(", "))))
    }
}
