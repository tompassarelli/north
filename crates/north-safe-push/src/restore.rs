use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::scans::outgoing::exit_status;
use crate::shell::git::default_branch;
use crate::shell::origin::{self, Snapshot};
use north_landing::landing::step::{Refused, Step, git_quiet};

use north_landing::land::telemetry;

const USAGE: &str = "usage: safe-push --restore [--dry-run] COMMIT BRANCH\n\
republish a commit that is already public to a branch origin no longer has: COMMIT must be reachable from origin's default branch or from a ref in the lane-gc mirror (LANE_GC_MIRRORS, default ~/.local/state/agents/lane-gc/mirrors/REPO.git); BRANCH must be absent on origin and not the default branch. The content scans are skipped because the commits were published and scanned when first pushed.\n";

struct Request {
    dry_run: bool,
    oid: String,
    branch: String,
}

pub fn run(arguments: &[String]) -> u8 {
    let request = match parse(arguments) {
        Ok(Some(request)) => request,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprintln!("safe-push: {message}\n{USAGE}");
            return 2;
        }
    };
    match restore(Path::new("."), &request) {
        Ok(()) => 0,
        Err(Refused(code)) => code,
    }
}

fn parse(arguments: &[String]) -> Result<Option<Request>, String> {
    let mut dry_run = false;
    let mut positional = Vec::new();
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--dry-run" if !dry_run => dry_run = true,
            "--dry-run" => return Err("duplicate --dry-run".into()),
            other if other.starts_with('-') => return Err(format!("unknown argument '{other}'")),
            other => positional.push(other.to_owned()),
        }
    }
    let [oid, branch] = <[String; 2]>::try_from(positional).map_err(|_| "--restore needs COMMIT and BRANCH".to_owned())?;
    if !(matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())) {
        return Err(format!("'{oid}' is not a full lowercase commit id"));
    }
    Ok(Some(Request { dry_run, oid, branch }))
}

fn restore(directory: &Path, request: &Request) -> Step {
    let destination_ref = format!("refs/heads/{}", request.branch);
    if !git(directory, &["check-ref-format", &destination_ref]) || request.branch.starts_with('-') || request.branch == "HEAD" {
        return Err(say(format!("safe-push: invalid destination branch '{}'", request.branch)));
    }
    let default = default_branch(directory);
    if request.branch == default {
        return Err(say(format!("safe-push: --restore never touches origin's default branch {default}")));
    }
    let origin = git_quiet(directory, &["remote", "get-url", "--push", "origin"]).ok_or_else(|| say("safe-push: origin push URL unavailable"))?;
    let listing = git_quiet(directory, &["ls-remote", "--symref", &origin, &destination_ref]).ok_or_else(|| say(format!("safe-push: cannot read origin {destination_ref}")))?;
    match origin::snapshot(&listing, &destination_ref) {
        Ok(Snapshot::Absent) => {}
        Ok(Snapshot::Object(oid)) => return Err(say(format!("safe-push: origin {destination_ref} exists at {oid} — --restore only recreates a missing branch; NOT pushed"))),
        Err(refusal) => return Err(say(refusal.trim_end())),
    }
    let source = public_source(directory, &origin, &default, &request.oid)
        .ok_or_else(|| say(format!("safe-push: {} is not reachable from origin's {default} or a lane-gc mirror ref — --restore republishes only public commits; NOT pushed", request.oid)))?;
    let refspec = format!("{}:{destination_ref}", request.oid);
    let lease = format!("--force-with-lease={destination_ref}:");
    if request.dry_run {
        println!("safe-push: ✓ public commit (--dry-run) — would restore {refspec} from {} to origin", source.display());
        return Ok(());
    }
    println!("safe-push: restoring public commit {refspec} from {} to origin", source.display());
    let status = Command::new("git")
        .current_dir(&source)
        .env("SAFE_PUSH_ACTIVE", "1")
        .args(["push", "--no-follow-tags", "--no-verify", &lease, &origin, &refspec])
        .status();
    exit_status(status)?;
    match git_quiet(directory, &["ls-remote", &origin, &destination_ref]) {
        Some(listing) if listing.split('\t').next() == Some(request.oid.as_str()) => {
            println!("safe-push: restored {} to {}", request.oid, request.branch);
            Ok(())
        }
        _ => Err(say(format!("safe-push: NOT restored: origin {destination_ref} is not at {}", request.oid))),
    }
}

fn public_source(directory: &Path, origin: &str, default: &str, oid: &str) -> Option<PathBuf> {
    let commit = format!("{oid}^{{commit}}");
    let mirror = mirror(&telemetry::repository(origin));
    if mirror.is_dir() && git(&mirror, &["cat-file", "-e", &commit]) {
        let held = git_quiet(&mirror, &["for-each-ref", "--contains", oid, "--format=%(refname)", "refs/archive", &format!("refs/heads/{default}")]);
        if held.is_some_and(|refs| !refs.is_empty()) {
            return Some(mirror);
        }
    }
    let tip = format!("refs/remotes/origin/{default}");
    (git(directory, &["cat-file", "-e", &commit]) && git(directory, &["merge-base", "--is-ancestor", oid, &tip])).then(|| directory.to_path_buf())
}

fn mirror(repository: &str) -> PathBuf {
    let root = std::env::var_os("LANE_GC_MIRRORS")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/lane-gc/mirrors"), PathBuf::from);
    root.join(format!("{repository}.git"))
}

fn say(message: impl std::fmt::Display) -> Refused {
    eprintln!("{message}");
    Refused(1)
}

fn git(directory: &Path, arguments: &[&str]) -> bool {
    git_quiet(directory, arguments).is_some()
}
