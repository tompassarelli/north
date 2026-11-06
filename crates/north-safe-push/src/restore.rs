use std::path::{Path, PathBuf};

use north_base::foundation::step::{Refused, Step};

use crate::shell::Shell;
use crate::shell::origin::{Hooks, Push, Snapshot, snapshot};

const USAGE: &str = "usage: safe-push --restore [--dry-run] COMMIT BRANCH\n\
republish a commit that is already public to a branch origin no longer has: COMMIT must be reachable from origin's default branch or from a ref in the lane-gc mirror (LANE_GC_MIRRORS, default ~/.local/state/agents/lane-gc/mirrors/REPO.git); BRANCH must be absent on origin and not the default branch. The content scans are skipped because the commits were published and scanned when first pushed.\n";


struct Request {
    dry_run: bool,
    oid: String,
    branch: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Source {
    Mirror(PathBuf),
    Worktree,
}

impl Source {
    fn directory(&self) -> Option<&Path> {
        match self {
            Self::Mirror(mirror) => Some(mirror),
            Self::Worktree => None,
        }
    }

    fn display(&self) -> std::path::Display<'_> {
        self.directory().unwrap_or(Path::new(".")).display()
    }
}

pub fn source(holding_mirror: Option<PathBuf>, on_default: bool) -> Option<Source> {
    holding_mirror.map(Source::Mirror).or_else(|| on_default.then_some(Source::Worktree))
}

pub fn run(arguments: &[String], shell: &Shell) -> u8 {
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
    match restore(shell, &request) {
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

fn restore(shell: &Shell, request: &Request) -> Step {
    let git = &*shell.git;
    let destination_ref = format!("refs/heads/{}", request.branch);
    if !git.valid_branch_name(&request.branch) || request.branch.starts_with('-') || request.branch == "HEAD" {
        return Err(say(format!("safe-push: invalid destination branch '{}'", request.branch)));
    }
    let default = git.default_branch();
    if request.branch == default {
        return Err(say(format!("safe-push: --restore never touches origin's default branch {default}")));
    }
    let origin = git.push_url().ok_or_else(|| say("safe-push: origin push URL unavailable"))?;
    let listing = shell.origin.symref(&origin, &destination_ref).ok_or_else(|| say(format!("safe-push: cannot read origin {destination_ref}")))?;
    match snapshot(&listing, &destination_ref) {
        Ok(Snapshot::Absent) => {}
        Ok(Snapshot::Object(oid)) => return Err(say(format!("safe-push: origin {destination_ref} exists at {oid} — --restore only recreates a missing branch; NOT pushed"))),
        Err(refusal) => return Err(say(refusal.trim_end())),
    }
    let mirror = shell.settings.mirror(&origin);
    let holding_mirror = (shell.files.is_directory(&mirror) && git.mirror_holds(&mirror, &request.oid, &["refs/archive", &format!("refs/heads/{default}")])).then_some(mirror);
    let on_default = git.has_commit(&request.oid) && git.is_ancestor(&request.oid, &format!("refs/remotes/origin/{default}"));
    let source = source(holding_mirror, on_default)
        .ok_or_else(|| say(format!("safe-push: {} is not reachable from origin's {default} or a lane-gc mirror ref — --restore republishes only public commits; NOT pushed", request.oid)))?;
    let refspec = format!("{}:{destination_ref}", request.oid);
    let lease = format!("--force-with-lease={destination_ref}:");
    if request.dry_run {
        println!("safe-push: ✓ public commit (--dry-run) — would restore {refspec} from {} to origin", source.display());
        return Ok(());
    }
    println!("safe-push: restoring public commit {refspec} from {} to origin", source.display());
    shell.origin.push(&Push { source: source.directory(), url: &origin, refspec: &refspec, lease: Some(&lease), hooks: Hooks::Skip, claimed: "" })?;
    match shell.origin.tip(&origin, &destination_ref) {
        Some(oid) if oid == request.oid => {
            println!("safe-push: restored {} to {}", request.oid, request.branch);
            Ok(())
        }
        _ => Err(say(format!("safe-push: NOT restored: origin {destination_ref} is not at {}", request.oid))),
    }
}

fn say(message: impl std::fmt::Display) -> Refused {
    eprintln!("{message}");
    Refused(1)
}
