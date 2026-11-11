use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Percent(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unix(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Usd(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seconds(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModelFamily {
    Fable,
    Opus,
    Sonnet,
    Haiku,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Window {
    FiveHour,
    Week,
    Scoped(ModelFamily),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Stream,
    Header,
    Cli,
    Web,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub row: RowId,
    pub account: Label,
    pub window: Window,
    pub used: Percent,
    pub resets_at: Option<Unix>,
    pub read_at: Unix,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LimitEvent {
    pub account: Label,
    pub window: Window,
    pub resets_at: Option<Unix>,
    pub at: Unix,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub provider: Provider,
    pub label: Label,
    pub weight: f64,
    pub signed_in: BTreeSet<Host>,
    pub extra_usage_enabled: Option<bool>,
    pub extra_usage_spend: Option<Usd>,
    pub rates: Rates,
    pub credit: Option<Credit>,
}

/// An API organization's prepaid grant: an account that carries one bills per token and has no plan windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Credit {
    pub left: Usd,
    pub next_grant: Option<Unix>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub five_hour: Rate,
    pub week: Rate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rate {
    Account { points_per_usd: f64, n: u32 },
    Pooled { points_per_usd: f64, n: u32 },
}

impl Rate {
    pub fn points_per_usd(self) -> f64 {
        match self {
            Self::Account { points_per_usd, .. } | Self::Pooled { points_per_usd, .. } => points_per_usd,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Fleet,
    AgentTool { tool_use_id: String },
    Tom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Worker,
    Lead,
    Proxy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub name: Name,
    pub owner: Owner,
    pub account: Label,
    pub family: ModelFamily,
    pub standing: Standing,
    pub parked_since: Option<Unix>,
}

#[derive(Debug, Clone, Copy)]
pub struct World<'a> {
    pub now: Unix,
    pub accounts: &'a [Account],
    pub readings: &'a [Reading],
    pub limits: &'a [LimitEvent],
    pub sessions: &'a [Session],
}
