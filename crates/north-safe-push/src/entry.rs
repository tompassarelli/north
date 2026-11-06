use std::path::Path;

use crate::gates::run as gates;
use crate::landing::{candidate, preflight, route};
use crate::landing::south_ledger;
use crate::report::terminal;
use crate::publication::run as publication;
use crate::request::{self, Arguments, Plan};
use crate::restore;
use crate::shell::Shell;
use crate::tags::{publish, release};

pub use crate::landing::south_ledger::HostFacts;

pub const USAGE: &str = request::USAGE;

pub fn run(arguments: &[String], host: &HostFacts) -> u8 {
    let shell = Shell::live(Path::new("."));
    match arguments {
        [verb, rest @ ..] if verb == "abandon" => (host.abandon)(rest),
        [flag, rest @ ..] if flag == "--restore" => restore::run(rest, &shell),
        [flag, rest @ ..] if flag == "--enqueue" => route::ticket(rest),
        [flag, base, head] if flag == "--publication-scan" => gates::publication_scan(base, head),
        [flag, ..] if flag == "--publication-scan" => {
            eprint!("safe-push: --publication-scan needs BASE HEAD and nothing else\n{USAGE}");
            2
        }
        [flag, base_flag, base] if flag == "--check-candidate" && base_flag == "--base" && is_object_id(base) => {
            candidate::run(base, &shell)
        }
        [flag, ..] if flag == "--check-candidate" => {
            eprint!("safe-push: --check-candidate needs --base SHA and nothing else\n{USAGE}");
            2
        }
        _ => dispatch(arguments, &shell),
    }
}

fn dispatch(arguments: &[String], shell: &Shell) -> u8 {
    let Ok(Plan::Push(request)) = request::arguments(arguments) else { return publish::run(arguments, shell) };
    if request.tag.is_some() || request.delete_tag.is_some() {
        return publish::run(arguments, shell);
    }
    match default_destination(&request, shell) {
        Some(to) if request.dry_run => preflight::run(&request, &to, shell),
        Some(to) => route::land(arguments, &to, shell),
        None => publication::run(arguments, shell),
    }
}

fn default_destination(request: &Arguments, shell: &Shell) -> Option<String> {
    let default = shell.git.default_branch();
    let to = request.to.clone().or_else(|| shell.git.current_branch())?;
    (to == default).then_some(to)
}

fn is_object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn safe_push(arguments: &[String], host: &HostFacts) -> u8 {
    match south_ledger::ambient_route(arguments, host) {
        Some(route) => south_ledger::land(&route),
        None => run(arguments, host),
    }
}

pub fn release(arguments: &[String]) -> u8 {
    release::run(arguments, &Shell::live(Path::new(".")))
}

pub fn land(arguments: &[String]) -> Option<u8> {
    let (verb, rest) = arguments.split_first()?;
    match verb.as_str() {
        "push" => Some(publication::run(rest, &Shell::live(Path::new(".")))),
        "route" => Some(route::run(rest)),
        "restore" => Some(restore::run(rest, &Shell::live(Path::new(".")))),
        _ => None,
    }
}

pub fn remote_land(arguments: &[String], host: &HostFacts) -> u8 {
    south_ledger::run(arguments, host)
}

pub fn safe_push_result(arguments: &[String]) -> u8 {
    terminal::run(arguments)
}
