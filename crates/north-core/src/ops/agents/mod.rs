mod busy;
mod lines;
pub mod plan;
mod replay;
pub mod script;
mod status;
mod tiers;
#[cfg(test)]
mod tests;

pub const USAGE: &str = "usage: agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|busy|presence [--json] [--at ISO]|flow [--json] [--repo NAME]... [--stall-alarm]|plan [--score PROJECT] [--place BRIEF...|--credit] [--quote --project P --boxes N [--category C]] [--json] [--mode split|claude-only] [--at SCENARIO.json]|routines [list|show NAME|pointer NAME] [--crons FILE|-]|report|usage [--refresh] [--json] [--gate PROVIDER] [--fast PROVIDER [--domain D] [--cached]]|news [--days 7] [--noise ID|--more ID] [--feedback PATH]}\nagents pace grant ACCOUNT MAX-WORKERS --until UTC --for LEAD --reason TEXT [--task NAME --uses N]";

#[derive(Debug, Eq, PartialEq)]
pub enum Verb {
    Catalog,
    Sync,
    Pace,
    Plan,
    Busy,
    Script(&'static str),
    Command(&'static str),
}

impl Verb {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "status" | "stale" | "inspect" | "path" | "on" | "off" => Self::Catalog,
            "sync" => Self::Sync,
            "pace" => Self::Pace,
            "plan" => Self::Plan,
            "busy" => Self::Busy,
            "usage" => Self::Script("usage.py"),
            "presence" => Self::Script("presence.py"),
            "flow" => Self::Command("flow"),
            "news" => Self::Command("news"),
            "routines" => Self::Script("routines.py"),
            "report" => Self::Command("report"),
            _ => return None,
        })
    }
}

pub fn busy(arguments: &[String]) -> u8 {
    busy::run(arguments)
}
