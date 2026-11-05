use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use super::git::merge_base;
use crate::land::after::follow::program;

const USAGE: &str = "usage: north land scan language --base BASE --head HEAD [--origin URL] [--safe-push PATH]";
pub const STOP: u8 = 69;

#[derive(Default)]
struct Arguments {
    base: String,
    head: String,
    origin: String,
    safe_push: String,
}

fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next()?.clone();
        match flag.as_str() {
            "--base" => parsed.base = value,
            "--head" => parsed.head = value,
            "--origin" => parsed.origin = value,
            "--safe-push" => parsed.safe_push = value,
            _ => return None,
        }
    }
    (!parsed.head.is_empty()).then_some(parsed)
}

struct Gates {
    root: String,
    base: String,
    head: String,
}

impl Gates {
    fn gate(&self, subcommand: &str) -> u8 {
        let status = Command::new(program())
            .args([subcommand, "--repo", &self.root, "--base", &self.base, "--head", &self.head])
            .stdin(Stdio::null())
            .status();
        status.map_or(127, code)
    }

    fn refused(&self, subcommand: &str, label: &str) -> Option<u8> {
        let status = self.gate(subcommand);
        (status != 0).then(|| {
            eprintln!("safe-push: {label} refused the landing; NOT pushed.");
            status
        })
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(arguments) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let Some(root) = git(None, &["rev-parse", "--show-toplevel"]) else {
        eprintln!("safe-push: the language gate cannot find the repository root; NOT pushed.");
        return 1;
    };
    let base = if arguments.base.is_empty() { git(None, &["hash-object", "-t", "tree", "/dev/null"]).unwrap_or_default() } else { merge_base(Path::new(&root), &arguments.base, &arguments.head) };
    let gates = Gates { root, base, head: arguments.head.clone() };
    for subcommand in ["language-gate", "build-cache-gate"] {
        if lacks(subcommand) {
            let needed = needed(&arguments.safe_push);
            eprintln!("safe-push: this host's north runtime lacks north {subcommand}, which safe-push {needed} calls; run north runtime update to {needed} or later; NOT pushed.");
            return STOP;
        }
    }
    if let Some(status) = gates.refused("language-gate", "language gate").or_else(|| gates.refused("build-cache-gate", "build-cache gate")) {
        return status;
    }
    if !lacks("runtime-floor-gate")
        && let Some(status) = gates.refused("runtime-floor-gate", "runtime-floor gate")
    {
        return status;
    }
    bin_reference_gate(&gates, &arguments.origin)
}

fn bin_reference_gate(gates: &Gates, origin: &str) -> u8 {
    if !(origin.ends_with("tompassarelli/north") || origin.ends_with("tompassarelli/north.git")) {
        return 0;
    }
    let deleted = git(Some(&gates.root), &["diff", "--no-renames", "--name-only", "--diff-filter=D", &gates.base, &gates.head, "--", "bin"]);
    if deleted.is_none_or(|paths| paths.is_empty()) {
        return 0;
    }
    let offered = Command::new(program()).args(["bin-references", "--help"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    if !offered.is_ok_and(|status| status.success()) {
        eprintln!("safe-push: this host's north runtime lacks north bin-references, so a deleted bin/ entry cannot be checked against deployed units; run north runtime update; NOT pushed.");
        return 1;
    }
    gates.gate("bin-references")
}

fn lacks(subcommand: &str) -> bool {
    let Ok(output) = Command::new(program()).args([subcommand, "--help"]).stdin(Stdio::null()).output() else {
        return false;
    };
    let unrecognized = format!("unrecognized command '{subcommand}");
    String::from_utf8_lossy(&output.stdout).contains(&unrecognized) || String::from_utf8_lossy(&output.stderr).contains(&unrecognized)
}

fn needed(safe_push: &str) -> String {
    let directory = Path::new(safe_push).parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    git(Some(&directory.to_string_lossy()), &["rev-parse", "--short=12", "HEAD"]).filter(|oid| !oid.is_empty()).unwrap_or_else(|| "this safe-push".to_owned())
}

fn git(directory: Option<&str>, arguments: &[&str]) -> Option<String> {
    super::git::text(Path::new(directory.unwrap_or(".")), arguments).map(|text| text.trim_end().to_owned())
}

fn code(status: ExitStatus) -> u8 {
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}
