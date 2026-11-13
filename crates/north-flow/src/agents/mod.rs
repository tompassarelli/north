mod busy;
mod goal;
mod lines;
pub mod plan;
mod replay;
pub mod routines;
mod status;
mod tiers;

pub const USAGE: &str = "usage: agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|busy|plan [--score PROJECT] [--place BRIEF...|--credit|--roomiest] [--quote --project P --boxes N [--category C]] [--json] [--mode split|claude-only] [--at SCENARIO.json]|routines [list|show NAME|pointer NAME] [--crons FILE|-]}";

#[derive(Debug, Eq, PartialEq)]
pub enum Verb {
    Catalog,
    Sync,
    Plan,
    Busy,
    Routines,
}

impl Verb {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "status" | "stale" | "inspect" | "path" | "on" | "off" | "check-hook-paths" | "check-owners" | "live-refs" => Self::Catalog,
            "sync" => Self::Sync,
            "plan" => Self::Plan,
            "busy" => Self::Busy,
            "routines" => Self::Routines,
            _ => return None,
        })
    }
}

pub fn busy(arguments: &[String]) -> u8 {
    busy::run(arguments)
}
