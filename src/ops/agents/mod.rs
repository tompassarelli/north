mod busy;
pub(crate) mod plan;
pub(crate) mod script;
#[cfg(test)]
mod tests;

pub(crate) const USAGE: &str = "usage: agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|busy|presence [--json] [--at ISO]|flow [--json] [--repo NAME]... [--stall-alarm]|plan [--score PROJECT] [--place BRIEF...|--credit] [--quote --project P --boxes N [--category C]] [--json] [--mode split|claude-only] [--at SCENARIO.json]|routines [list|show NAME|pointer NAME] [--crons FILE|-]|report|usage [--refresh] [--json] [--gate PROVIDER] [--fast PROVIDER [--domain D] [--cached]]|news [--days 7] [--noise ID|--more ID] [--feedback PATH]}\nagents pace grant ACCOUNT MAX-WORKERS --until UTC --for LEAD --reason TEXT [--task NAME --uses N]";

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Verb {
    Catalog,
    Sync,
    Pace,
    Plan,
    Busy,
    Script(&'static str),
    Command(&'static str),
}

impl Verb {
    pub(crate) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "status" | "stale" | "inspect" | "path" | "on" | "off" => Self::Catalog,
            "sync" => Self::Sync,
            "pace" => Self::Pace,
            "plan" => Self::Plan,
            "busy" => Self::Busy,
            "usage" => Self::Script("usage.py"),
            "presence" => Self::Script("presence.py"),
            "flow" => Self::Command("flow"),
            "news" => Self::Script("news.py"),
            "routines" => Self::Script("routines.py"),
            "report" => Self::Command("report"),
            _ => return None,
        })
    }
}

pub(crate) fn busy(arguments: &[String]) -> u8 {
    busy::run(arguments)
}
