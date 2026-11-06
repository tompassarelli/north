use std::path::Path;

use north_base::foundation::step::{Refused, Step, cancelled, refuse};

use super::delete::{DeleteRequest, delete_tag};
use super::name::release_tag;
use crate::request::{self, Arguments, Plan, USAGE};
use crate::session::cancel::watch_for_cancel;
use crate::session::identity::{Identity, OriginRefs, remote_snapshot, state_changed};
use crate::session::output_log::claim_output_log;
use crate::gates::tag::{self as tag_scan, TagScan};
use crate::shell::Shell;
use crate::shell::origin::{Hooks, Push};

pub struct TagRequest<'a> {
    pub tag: &'a str,
    pub dry_run: bool,
}

pub fn run(arguments: &[String], shell: &Shell) -> u8 {
    watch_for_cancel();
    let outcome = match request::arguments(arguments) {
        Ok(Plan::Help) => {
            print!("{USAGE}");
            return 0;
        }
        Ok(Plan::Push(Arguments { tag: Some(tag), .. })) if release_tag(&tag) => {
            eprintln!("safe-push: version tag {tag} publishes only through north release, which also creates its GitHub release; NOT pushed.");
            Err(Refused(2))
        }
        Ok(Plan::Push(Arguments { tag: Some(tag), dry_run, .. })) => match publish_tag(&TagRequest { tag: &tag, dry_run }, shell) {
            Ok(Scanned::Clean) => Ok(()),
            Ok(Scanned::Refused(Refused(code))) => return if cancelled().is_err() { 130 } else { code },
            Err(_) if cancelled().is_err() => Err(Refused(130)),
            Err(refused) => Err(refused),
        },
        Ok(Plan::Push(Arguments { delete_tag: Some(tag), dry_run, .. })) => delete_tag(&DeleteRequest { tag: &tag, dry_run }, shell),
        Ok(Plan::Push(_)) => {
            eprintln!("safe-push: north safe-push publishes tags only; land branches with bin/safe-push");
            Err(Refused(2))
        }
        Err(refusal) => {
            eprint!("{}", refusal.stderr());
            Err(Refused(refusal.exit_code()))
        }
    };
    terminal(outcome, shell.settings.raw_exit)
}

fn terminal(outcome: Step, raw_exit: bool) -> u8 {
    match outcome {
        Ok(()) => 0,
        Err(Refused(code)) if raw_exit => code,
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

pub fn publish_tag(request: &TagRequest, shell: &Shell) -> Step<Scanned> {
    let claims = claim_output_log()?;
    let dir = std::env::current_dir().map_err(|_| Refused(1))?;
    let git = &*shell.git;
    git.common_dir()?;
    let identity = Identity::capture(&dir)?;
    let tag_oid = git.resolve(&format!("refs/tags/{}", request.tag)).ok_or_else(|| refuse(format!("safe-push: no such tag '{}'", request.tag)))?;
    if git.object_type(&tag_oid).as_deref() != Some("tag") {
        return Err(refuse(format!("safe-push: tag '{}' is not annotated — refusing", request.tag)));
    }
    let target = git.commit_of(&tag_oid)?;
    if !shell.processes.on_path("betterleaks") {
        return Err(refuse("safe-push: betterleaks not on PATH — refusing to push without a secret scan"));
    }
    cancelled()?;
    let tagged = Tagged { dir: &dir, identity: &identity, request, tag_oid: &tag_oid, target: &target, claimed: &claims.list };
    Ok(match tagged.scan_and_push(shell) {
        Ok(()) => Scanned::Clean,
        Err(refused) => Scanned::Refused(refused),
    })
}

struct Tagged<'a> {
    dir: &'a Path,
    identity: &'a Identity,
    request: &'a TagRequest<'a>,
    tag_oid: &'a str,
    target: &'a str,
    claimed: &'a str,
}

impl Tagged<'_> {
    fn scan_and_push(&self, shell: &Shell) -> Step {
        let (dir, identity, git) = (self.dir, self.identity, &*shell.git);
        let tag_ref = format!("refs/tags/{}", self.request.tag);
        let origin = OriginRefs::fetch(dir, &identity.origin_url)?;
        let fetched = origin.snapshot()?;
        let origin_snapshot = remote_snapshot(dir, &identity.origin_url)?;
        if origin_snapshot != fetched {
            return Err(state_changed("origin refs changed while capturing tag scan base"));
        }
        let range = ScanRange::new(shell, self.target, origin.commits()?)?;
        let annotation = git.object(self.tag_oid)?;
        tag_scan::scan(&TagScan {
            directory: dir,
            root: &identity.root,
            origin_url: &identity.origin_url,
            tag: self.request.tag,
            annotation: &annotation,
            revisions: &range.revisions,
            base: &range.base,
            target: self.target,
            outgoing: range.outgoing,
        })?;
        identity.revalidate(dir)?;
        let current_tag = git.resolve(&tag_ref).ok_or_else(|| state_changed("tag disappeared"))?;
        if current_tag != self.tag_oid {
            return Err(state_changed("tag object changed"));
        }
        if git.commit_of(&current_tag)? != self.target {
            return Err(state_changed("tag target changed"));
        }
        if remote_snapshot(dir, &identity.origin_url)? != origin_snapshot {
            return Err(state_changed("origin refs changed"));
        }
        drop(origin);
        self.push(shell, &tag_ref)
    }

    fn push(&self, shell: &Shell, tag_ref: &str) -> Step {
        cancelled()?;
        if self.request.dry_run {
            println!("safe-push: ✓ clean (--dry-run) — would push tag {} -> origin", self.request.tag);
            return Ok(());
        }
        println!("safe-push: ✓ clean — pushing tag {} -> origin", self.request.tag);
        cancelled()?;
        let refspec = format!("{}:{tag_ref}", self.tag_oid);
        shell.origin.push(&Push { source: None, url: &self.identity.origin_url, refspec: &refspec, lease: None, hooks: Hooks::Run, claimed: self.claimed })?;
        cancelled()
    }
}

struct ScanRange {
    revisions: Vec<String>,
    base: String,
    outgoing: bool,
}

impl ScanRange {
    fn new(shell: &Shell, target: &str, origin_commits: Vec<String>) -> Step<Self> {
        let mut revisions = vec![target.to_owned()];
        revisions.extend(origin_commits.into_iter().map(|oid| format!("^{oid}")));
        let listing = shell.git.boundary(&revisions)?;
        let outgoing = shell.git.count(&revisions)? != "0";
        let base = if outgoing { listing.lines().find_map(|line| line.strip_prefix('-')).unwrap_or("").to_owned() } else { target.to_owned() };
        Ok(Self { revisions, base, outgoing })
    }
}
