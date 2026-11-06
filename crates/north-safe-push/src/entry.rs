use std::path::Path;

use crate::gates::run as gates;
use crate::landing::route;
use crate::landing::south_ledger;
use crate::report::terminal;
use crate::publication::run as publication;
use crate::request;
use crate::restore;
use crate::shell::Shell;
use crate::tags::{publish, release};

pub use crate::landing::south_ledger::HostFacts;

pub const USAGE: &str = request::USAGE;

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [flag, base, head] if flag == "--publication-scan" => gates::publication_scan(base, head),
        [flag, ..] if flag == "--publication-scan" => {
            eprint!("safe-push: --publication-scan needs BASE HEAD and nothing else\n{USAGE}");
            2
        }
        _ => publish::run(arguments),
    }
}

pub fn safe_push(arguments: &[String], host: &HostFacts) -> u8 {
    match south_ledger::ambient_route(arguments, host) {
        Some(route) => south_ledger::land(&route),
        None => run(arguments),
    }
}

pub fn release(arguments: &[String]) -> u8 {
    release::run(arguments)
}

pub fn land(arguments: &[String]) -> Option<u8> {
    let (verb, rest) = arguments.split_first()?;
    match verb.as_str() {
        "push" => Some(publication::run(rest, &Shell::live(Path::new(".")))),
        "route" => Some(route::run(rest)),
        "restore" => Some(restore::run(rest)),
        _ => None,
    }
}

pub fn remote_land(arguments: &[String], host: &HostFacts) -> u8 {
    south_ledger::run(arguments, host)
}

pub fn safe_push_result(arguments: &[String]) -> u8 {
    terminal::run(arguments)
}
