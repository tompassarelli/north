mod busy;
mod goal;
mod lines;
pub mod plan;
mod replay;
pub mod routines;
mod status;
mod tiers;

pub const USAGE: &str = "usage: agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|busy|plan [--score PROJECT] [--place BRIEF...|--credit] [--quote --project P --boxes N [--category C]] [--json] [--mode split|claude-only] [--at SCENARIO.json]|routines [list|show NAME|pointer NAME] [--crons FILE|-]|usage [--refresh [--local]] [--write-pace-gate] [--json] [--gate PROVIDER] [--fast PROVIDER [--domain D] [--cached]]}";

#[derive(Debug, Eq, PartialEq)]
pub enum Verb {
    Catalog,
    Sync,
    Plan,
    Busy,
    Routines,
    Usage,
}

impl Verb {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "status" | "stale" | "inspect" | "path" | "on" | "off" => Self::Catalog,
            "sync" => Self::Sync,
            "plan" => Self::Plan,
            "busy" => Self::Busy,
            "usage" => Self::Usage,
            "routines" => Self::Routines,
            _ => return None,
        })
    }
}

pub fn busy(arguments: &[String]) -> u8 {
    busy::run(arguments)
}
