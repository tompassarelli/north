use toml_edit::DocumentMut;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Tier {
    Haiku,
    Sonnet,
    Opus,
}

impl Tier {
    /// The `[tiers]` key in South's orchestration, which is also the ledger's tier label.
    pub fn key(self) -> &'static str {
        match self {
            Self::Haiku => "claude:haiku",
            Self::Sonnet => "claude:sonnet-high",
            Self::Opus => "claude:opus-medium",
        }
    }

    /// The name the workers skill staffs it by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Haiku => "worker-haiku",
            Self::Sonnet => "sonnet-high",
            Self::Opus => "worker",
        }
    }

    /// The word a ledger tier label carries for it.
    pub fn family(self) -> &'static str {
        match self {
            Self::Haiku => "haiku",
            Self::Sonnet => "sonnet",
            Self::Opus => "opus",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Haiku, Self::Sonnet, Self::Opus].into_iter().find(|tier| tier.key() == text || tier.name() == text)
    }

    pub fn model_effort(self, cfg: &DocumentMut) -> Result<(String, String), String> {
        let table = cfg.get("tiers").and_then(|tiers| tiers.get(self.key()));
        let field = |name: &str| table.and_then(|tier| tier.get(name)).and_then(toml_edit::Item::as_str).map(str::to_owned);
        field("model").zip(field("effort")).ok_or_else(|| format!("orchestration has no model and effort for [tiers.\"{}\"]", self.key()))
    }

    pub fn serves(self, model: &str) -> bool {
        model.to_lowercase().contains(self.family())
    }
}

/// Every model tier a run can name, cheapest first: its key and the label the ledger's tier column holds for it.
pub const LADDER: [(&str, &str); 10] = [
    ("claude:haiku", "haiku"),
    ("codex:luna-low", "gpt-6-luna low"),
    ("claude:sonnet-high", "sonnet"),
    ("codex:sol-medium", "gpt-6.1-sol medium"),
    ("codex:sol-high", "gpt-6.1-sol high"),
    ("codex:sol-xhigh", "gpt-6.1-sol xhigh"),
    ("claude:opus-medium", "medium"),
    ("claude:opus-high", "high"),
    ("claude:opus-xhigh", "xhigh"),
    ("codex:astra", "gpt-6-astra xhigh"),
];

/// The ledger's tier label for a ladder key.
pub fn run_label(key: &str) -> Option<&'static str> {
    LADDER.iter().find(|(rung, _)| *rung == key).map(|(_, label)| *label)
}

/// The ladder key a ledger tier label names.
pub fn label_key(label: &str) -> Option<&'static str> {
    LADDER.iter().find(|(_, rung)| *rung == label).map(|(key, _)| *key)
}

/// A ladder key's place from the cheapest, past the top for a key off the ladder.
pub fn rank(key: &str) -> usize {
    LADDER.iter().position(|(rung, _)| *rung == key).unwrap_or(LADDER.len())
}
