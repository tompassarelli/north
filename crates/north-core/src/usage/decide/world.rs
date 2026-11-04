use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Workstream(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoxId(pub String);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Fleet,
    AgentTool { tool_use_id: String },
    Tom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub name: Name,
    pub owner: Owner,
    pub account: Label,
    pub family: ModelFamily,
    pub workstream: Option<Workstream>,
    pub box_id: Option<BoxId>,
    pub lead: bool,
    pub spent: Option<Usd>,
    pub active_minutes: f64,
    pub expected: Expected,
    pub parked_since: Option<Unix>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub p50: Usd,
    pub p90: Usd,
    pub minutes: f64,
    pub source: EstimateSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EstimateSource {
    Cell(String),
    CategoryPool(String),
    TierMedian(String),
    BriefEta { minutes: f64 },
    Budget(Usd),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub provider: Provider,
    pub family: ModelFamily,
    pub owner: Owner,
    pub workstream: Option<Workstream>,
    pub expected: Expected,
    pub pin: Option<Label>,
    pub host: Option<Host>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Credit {
    pub remaining: Usd,
    pub expires_at: Unix,
    pub read_at: Unix,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub provider: Provider,
    pub until: Unix,
    pub gate: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clear {
    pub workstream: Workstream,
    pub p90: Usd,
    pub by: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub reserve: BTreeMap<Provider, Percent>,
    pub crossings: Vec<Crossing>,
    pub clears: Vec<Clear>,
}

#[derive(Debug, Clone, Copy)]
pub struct World<'a> {
    pub now: Unix,
    pub accounts: &'a [Account],
    pub readings: &'a [Reading],
    pub limits: &'a [LimitEvent],
    pub sessions: &'a [Session],
    pub credit: Option<Credit>,
    pub settings: &'a Settings,
}
