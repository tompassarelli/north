use std::path::Path;

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
    publish::run(arguments)
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
        "push" => Some(publication::run(rest, &Shell::live(Path::new(".")), &LandingChecks)),
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

struct LandingChecks;

impl publication::Checks for LandingChecks {
    fn scan(&self, arguments: &[String]) -> u8 {
        north_landing::land::scans::run(arguments)
    }

    fn sync_dependencies(&self, root: &Path) -> Result<(), String> {
        north_landing::land::slots::deps::sync(root)
    }
}
