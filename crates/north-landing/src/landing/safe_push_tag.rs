use std::fs::File;
use std::os::fd::AsFd;
use std::path::Path;
use std::process::Command;

use super::safe_push_guards::{self as guards, Denylist};
use super::safe_push_plan::{self, Arguments, Plan, USAGE};
use super::safe_push_state::{
    Identity, OriginRefs, Refused, Step, watch_for_cancel, cancelled, git_quiet, git_read, refuse, remote_snapshot, state_changed,
};

pub struct TagRequest<'a> {
    pub tag: &'a str,
    pub dry_run: bool,
}

pub fn run(arguments: &[String]) -> u8 {
    watch_for_cancel();
    let outcome = match safe_push_plan::arguments(arguments) {
        Ok(Plan::Help) => {
            print!("{USAGE}");
            return 0;
        }
        Ok(Plan::Push(Arguments { tag: Some(tag), dry_run, .. })) => match publish_tag(&TagRequest { tag: &tag, dry_run }) {
            Ok(Scanned::Clean) => Ok(()),
            Ok(Scanned::Refused(Refused(code))) => return if cancelled().is_err() { 130 } else { code },
            Err(_) if cancelled().is_err() => Err(Refused(130)),
            Err(refused) => Err(refused),
        },
        Ok(Plan::Push(_)) => {
            eprintln!("safe-push: north safe-push publishes tags only; land branches with bin/safe-push");
            Err(Refused(2))
        }
        Err(refusal) => {
            eprint!("{}", refusal.stderr());
            Err(Refused(refusal.exit_code()))
        }
    };
    terminal(outcome)
}

fn terminal(outcome: Step) -> u8 {
    match outcome {
        Ok(()) => 0,
        Err(Refused(code)) if std::env::var_os("NORTH_NO_AUTOREAP").is_some_and(|value| value == "1") => code,
        Err(Refused(130 | 143)) => {
            println!("safe-push: cancelled");
            130
        }
        Err(Refused(_)) => {
            println!("safe-push: refused safe-push did not land");
            1
        }
    }
}

pub enum Scanned {
    Clean,
    Refused(Refused),
}

pub fn publish_tag(request: &TagRequest) -> Step<Scanned> {
    let claims = claim_output_log()?;
    let dir = std::env::current_dir().map_err(|_| Refused(1))?;
    git_read(&dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let identity = Identity::capture(&dir)?;
    let tag_oid = git_quiet(&dir, &["rev-parse", "-q", "--verify", &format!("refs/tags/{}", request.tag)])
        .ok_or_else(|| refuse(format!("safe-push: no such tag '{}'", request.tag)))?;
    if git_quiet(&dir, &["cat-file", "-t", &tag_oid]).as_deref() != Some("tag") {
        return Err(refuse(format!("safe-push: tag '{}' is not annotated — refusing", request.tag)));
    }
    let target = git_read(&dir, &["rev-list", "-n1", &tag_oid])?;
    if !guards::on_path("gitleaks") {
        return Err(refuse("safe-push: gitleaks not on PATH — refusing to push without a secret scan"));
    }
    cancelled()?;
    Ok(match scan_and_push(&dir, &identity, request, &tag_oid, &target, &claims.list) {
        Ok(()) => Scanned::Clean,
        Err(refused) => Scanned::Refused(refused),
    })
}

fn scan_and_push(dir: &Path, identity: &Identity, request: &TagRequest, tag_oid: &str, target: &str, claimed: &str) -> Step {
    let tag_ref = format!("refs/tags/{}", request.tag);
    let origin = OriginRefs::fetch(dir, &identity.origin_url)?;
    let fetched = origin.snapshot()?;
    let origin_snapshot = remote_snapshot(dir, &identity.origin_url)?;
    if origin_snapshot != fetched {
        return Err(state_changed("origin refs changed while capturing tag scan base"));
    }
    let scan = ScanRange::new(dir, target, origin.commits()?)?;
    scan_tag(dir, identity, request.tag, tag_oid, &scan)?;
    identity.revalidate(dir)?;
    let current_tag = git_quiet(dir, &["rev-parse", "-q", "--verify", &tag_ref]).ok_or_else(|| state_changed("tag disappeared"))?;
    if current_tag != tag_oid {
        return Err(state_changed("tag object changed"));
    }
    if git_read(dir, &["rev-list", "-n1", &current_tag])? != target {
        return Err(state_changed("tag target changed"));
    }
    if remote_snapshot(dir, &identity.origin_url)? != origin_snapshot {
        return Err(state_changed("origin refs changed"));
    }
    drop(origin);
    push(dir, &identity.origin_url, request, tag_oid, &tag_ref, claimed)
}

struct ScanRange {
    revisions: Vec<String>,
    base: String,
    target: String,
    outgoing: bool,
}

impl ScanRange {
    fn new(dir: &Path, target: &str, origin_commits: Vec<String>) -> Step<Self> {
        let mut revisions = vec![target.to_owned()];
        revisions.extend(origin_commits.into_iter().map(|oid| format!("^{oid}")));
        let mut boundary = vec!["rev-list", "--boundary"];
        boundary.extend(revisions.iter().map(String::as_str));
        let listing = git_read(dir, &boundary)?;
        let mut base = listing.lines().find_map(|line| line.strip_prefix('-')).unwrap_or("").to_owned();
        let mut count = vec!["rev-list", "--count"];
        count.extend(revisions.iter().map(String::as_str));
        let outgoing = git_read(dir, &count)? != "0";
        if !outgoing {
            base = target.to_owned();
        }
        Ok(Self { revisions, base, target: target.to_owned(), outgoing })
    }
}

fn scan_tag(dir: &Path, identity: &Identity, tag: &str, tag_oid: &str, scan: &ScanRange) -> Step {
    guards::language_gate(&identity.root, &scan.base, &scan.target)?;
    guards::instruction_limits(&identity.root, &scan.target)?;
    let annotation = Command::new("git").current_dir(dir).args(["cat-file", "-p", tag_oid]).output().map_err(|_| Refused(127))?;
    let annotation_text = String::from_utf8_lossy(&annotation.stdout);
    guards::native_ref_scan(dir, &[tag, annotation_text.trim_end_matches('\n')], &scan.revisions)?;
    if scan.outgoing {
        guards::history_secret_scan(dir, &scan.revisions)?;
        guards::commit_email_scan(dir, &scan.revisions)?;
    }
    Denylist::from_env().scan(&identity.root, &identity.origin_url, &scan.base, &scan.target, tag)?;
    guards::annotation_secret_scan(dir, &annotation.stdout)
}

fn push(dir: &Path, origin_url: &str, request: &TagRequest, tag_oid: &str, tag_ref: &str, claimed: &str) -> Step {
    cancelled()?;
    if request.dry_run {
        println!("safe-push: ✓ clean (--dry-run) — would push tag {} -> origin", request.tag);
        return Ok(());
    }
    println!("safe-push: ✓ clean — pushing tag {} -> origin", request.tag);
    cancelled()?;
    let refspec = format!("{tag_oid}:{tag_ref}");
    let status = Command::new("git")
        .current_dir(dir)
        .env("SAFE_PUSH_ACTIVE", "1")
        .env("SAFE_PUSH_OUTPUT_CLAIMED", claimed)
        .args(["push", "--no-follow-tags", origin_url, &refspec])
        .status();
    guards::exit_status(status)?;
    cancelled()
}

struct Claims {
    list: String,
    _files: Vec<File>,
}

fn claim_output_log() -> Step<Claims> {
    let claimed_before = std::env::var("SAFE_PUSH_OUTPUT_CLAIMED").unwrap_or_default();
    let mut claims: Vec<(String, File)> = Vec::new();
    for fd in [1, 2] {
        let Ok(target) = std::fs::read_link(format!("/proc/self/fd/{fd}")) else { continue };
        let target = target.to_string_lossy().into_owned();
        let known = format!(" {} {claimed_before} ", claims.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>().join(" "));
        if !Path::new(&target).is_file() || known.contains(&format!(" {target} ")) {
            continue;
        }
        let Ok(file) = File::open(&target) else { return Err(Refused(1)) };
        if rustix::fs::flock(file.as_fd(), rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err() {
            let cwd = std::env::current_dir().unwrap_or_default();
            eprintln!(
                "safe-push: refusing in {}: {target} is already the log of another running safe-push; give each landing its own log file. NOT pushed.",
                cwd.display()
            );
            return Err(Refused(1));
        }
        claims.push((target, file));
    }
    let added: String = claims.iter().map(|(path, _)| format!(" {path}")).collect();
    Ok(Claims { list: format!("{claimed_before}{added}"), _files: claims.into_iter().map(|(_, file)| file).collect() })
}
