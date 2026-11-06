use crate::landing::route;
use crate::publication::run as publication;
use crate::request::{self, Arguments, Plan};
use crate::restore;
use crate::tags::{publish, release};

pub const USAGE: &str = request::USAGE;

pub struct Destination {
    pub to: String,
    pub dry_run: bool,
}

pub fn run(arguments: &[String]) -> u8 {
    publish::run(arguments)
}

pub fn release(arguments: &[String]) -> u8 {
    release::run(arguments)
}

pub fn land(arguments: &[String]) -> Option<u8> {
    let (verb, rest) = arguments.split_first()?;
    match verb.as_str() {
        "push" => Some(publication::run(rest)),
        "route" => Some(route::run(rest)),
        "restore" => Some(restore::run(rest)),
        _ => None,
    }
}

pub fn destination(arguments: &[String]) -> Option<Destination> {
    let Ok(Plan::Push(Arguments { tag: None, to: Some(to), dry_run, .. })) = request::arguments(arguments) else { return None };
    Some(Destination { to, dry_run })
}
