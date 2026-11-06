use std::path::{Path, PathBuf};

use crate::platform::betterleaks;
use crate::landing::step::{Refused, Step, exit_code, git_read};

pub(crate) mod denylist;
pub(crate) mod email;
pub(crate) mod language;
pub(crate) mod native_ref;
pub(crate) mod secrets;

use self::denylist::Denylist;
use self::email::commit_email_scan;
use self::native_ref::native_ref_scan;

const BETTERLEAKS_MISSING: &str = "safe-push: betterleaks not on PATH — refusing to publish the land check branch without a secret scan; NOT pushed.";
const SECRET_FOUND: &str = "safe-push: ✗ SECRET DETECTED — fix the leak; NOT pushed.";

pub fn secrets(dir: &Path, range: &str) -> Step {
    exit_status(betterleaks::command("betterleaks").current_dir(dir).args(betterleaks::history_args(range)).status())
}

fn origin_url(dir: &Path) -> Step<String> {
    git_read(dir, &["remote", "get-url", "--push", "origin"]).map_err(|_| Refused(2))
}

fn top_level(dir: &Path) -> Step<PathBuf> {
    git_read(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from).map_err(|_| Refused(2))
}

pub fn denylist(dir: &Path, base: &str, head: &str, reference: &str) -> Step {
    let origin = origin_url(dir)?;
    Denylist::from_env().scan(&top_level(dir)?, &origin, base, head, reference)
}

pub fn publication(dir: &Path, base: &str, head: &str) -> Step {
    let origin = origin_url(dir)?;
    if !on_path("betterleaks") {
        eprintln!("{BETTERLEAKS_MISSING}");
        return Err(Refused(1));
    }
    let range = if base.is_empty() { head.to_owned() } else { format!("{base}..{head}") };
    if secrets(dir, &range).is_err() {
        eprintln!("{SECRET_FOUND}");
        return Err(Refused(1));
    }
    Denylist::from_env().scan(&top_level(dir)?, &origin, base, head, "main")?;
    commit_email_scan(dir, &[range])
}

pub fn enqueue_gate(root: &Path, base: &str, denylist: &Denylist, origin: &str, destination: &str) -> Result<(), String> {
    if on_path("betterleaks") {
        secrets(root, &format!("{base}..HEAD")).map_err(|_| "secret scan refused the landing".to_owned())?;
    } else {
        println!("safe-push: ⚠ betterleaks is not on PATH here, so this lane was not scanned for secrets before queueing; the lander's scan stays the gate.");
    }
    denylist.scan(root, origin, base, "HEAD", destination).map_err(|_| "deny-list scan refused the landing".to_owned())
}

struct NativeRefs {
    ranges: Vec<String>,
    names: Vec<String>,
}

fn native_refs(args: &[String]) -> Option<NativeRefs> {
    let mut parsed = NativeRefs { ranges: Vec::new(), names: Vec::new() };
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--range" => parsed.ranges.push(rest.next()?.clone()),
            "--" => {
                parsed.names.extend(rest.cloned());
                break;
            }
            _ => return None,
        }
    }
    Some(parsed)
}

fn dispatch(dir: &Path, command: &str, rest: &[String]) -> Option<Step> {
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    Some(match (command, rest.as_slice()) {
        ("denylist", [base, head, reference]) => denylist(dir, base, head, reference),
        ("commit-email", revisions) => commit_email_scan(dir, &revisions.iter().map(|revision| (*revision).to_owned()).collect::<Vec<_>>()),
        ("secrets", [range]) => secrets(dir, range),
        ("publication", [base, head]) => publication(dir, base, head),
        _ => return None,
    })
}

pub fn run(arguments: &[String]) -> Option<u8> {
    let (command, rest) = arguments.split_first()?;
    let dir = std::env::current_dir().ok()?;
    let outcome = if command == "native-ref" {
        native_refs(rest).map(|parsed| {
            let names: Vec<&str> = parsed.names.iter().map(String::as_str).collect();
            native_ref_scan(&dir, &names, &parsed.ranges)
        })
    } else {
        dispatch(&dir, command, rest)
    };
    outcome.map(|step| match step {
        Ok(()) => 0,
        Err(Refused(code)) => code,
    })
}

pub(crate) fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(program);
            candidate.is_file() && std::fs::metadata(&candidate).is_ok_and(|meta| {
                std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0
            })
        })
    })
}

pub(crate) fn instruction_limits(root: &Path, reference: &str) -> Step {
    match crate::landing::instruction_limits::check(root, reference) {
        0 => Ok(()),
        code => Err(Refused(code)),
    }
}

pub(crate) fn exit_status(status: std::io::Result<std::process::ExitStatus>) -> Step {
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Refused(exit_code(status))),
        Err(_) => Err(Refused(127)),
    }
}
