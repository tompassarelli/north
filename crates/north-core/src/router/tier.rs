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

    pub fn invocation(self, cfg: &DocumentMut) -> Result<Vec<String>, String> {
        self.model_effort(cfg).map(|(model, effort)| vec!["--model".into(), model, "--effort".into(), effort])
    }

    pub fn serves(self, model: &str) -> bool {
        let family = match self {
            Self::Haiku => "haiku",
            Self::Sonnet => "sonnet",
            Self::Opus => "opus",
        };
        model.to_lowercase().contains(family)
    }
}
