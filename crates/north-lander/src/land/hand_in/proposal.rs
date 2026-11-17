use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::enqueue::{default_branch, git};
use super::{Admit, EARLY_WAIT, Named, PROPOSAL_FILE, PROPOSALS, PROPOSED, REFUSED, Road, checkout, kept, literal, piped};
use crate::land::scans::outgoing::denylist::{Denylist, Stand};

pub(super) fn admit_checked_head(road: &impl Road, verb: &str, named: &Named, requested_message: &str, has_files: bool) -> Result<(), u8> {
    if !road.requires_check_only() {
        return Ok(());
    }
    let head = match git(&named.lane.root, &["rev-parse", "--verify", "HEAD^{commit}"]) {
        Ok(head) => head,
        Err(error) => {
            eprintln!("{verb}: ✗ {error}; NOT committed.");
            return Err(1);
        }
    };
    let uncommitted = has_files || has_uncommitted(&named.lane.root, &named.relative);
    let admit = Admit { lane: &named.lane, head: &head, relative: &named.relative, requested_message, uncommitted };
    if let Err(refusal) = road.admit(&admit) {
        eprintln!("{verb}: ✗ {refusal}; NOT committed, NOT queued.");
        return Err(1);
    }
    Ok(())
}

fn has_uncommitted(root: &Path, relative: &[String]) -> bool {
    let tracked_dirty = git(root, &["status", "--porcelain", "--untracked-files=no"]).map_or(true, |status| !status.is_empty());
    if tracked_dirty || relative.is_empty() {
        return tracked_dirty;
    }
    let named: Vec<&str> = relative.iter().map(String::as_str).collect();
    literal(root, &[&["status", "--porcelain", "--untracked-files=all", "--"][..], &named].concat()).map_or(true, |status| !status.is_empty())
}

pub(super) fn judge_message(session: &str) -> u8 {
    let message = io::read_to_string(io::stdin()).unwrap_or_default();
    let Some(root) = std::env::current_dir().ok().and_then(|directory| checkout(&directory)) else { return 1 };
    let Some(kept) = proposal_path(&root, session) else { return 1 };
    let _ = fs::remove_file(&kept);
    let destination = default_branch(&root);
    if message.trim().is_empty() || session.trim().is_empty() || session.contains('\n') || !git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).is_ok_and(|branch| branch != destination) {
        return 1;
    }
    let (Ok(origin), Ok(base)) = (git(&root, &["remote", "get-url", "--push", "origin"]), git(&root, &["rev-parse", "HEAD"])) else { return 1 };
    let Some(head) = message_only(&root, &base, &message) else { return 1 };
    let passed = std::cell::Cell::new(false);
    let on_pass = || {
        if fs::write(&kept, format!("{session}\n{message}")).is_ok() {
            println!("{PROPOSED}");
            passed.set(true);
            record_proposal(&root, "judged", &message);
        }
        let _ = io::Write::flush(&mut io::stdout());
    };
    let verdict = Denylist::from_env().verdict_once_scanned(&root, &origin, (&base, &head, &destination), EARLY_WAIT, &on_pass);
    if verdict.stand == Stand::Refuse && !passed.get() {
        println!("{REFUSED}");
    }
    u8::from(verdict.stand != Stand::Pass)
}

fn proposal_path(root: &Path, session: &str) -> Option<PathBuf> {
    kept::path(root, PROPOSAL_FILE, session)
}

pub(super) fn proposed_message(root: &Path) -> Option<String> {
    kept::for_session(root, PROPOSAL_FILE)
}

pub(super) fn record_proposal(root: &Path, event: &str, message: &str) {
    let digest: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(message.as_bytes())).iter().map(|byte| format!("{byte:02x}")).collect();
    let line = serde_json::json!({"at": north_base::platform::records_mirror::now(), "event": event, "lane": root, "message": digest});
    let journal = north_base::platform::records_mirror::state_home().join(PROPOSALS);
    let appended = journal.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::OpenOptions::new().create(true).append(true).open(&journal)).and_then(|mut file| io::Write::write_all(&mut file, format!("{line}\n").as_bytes()));
    if let Err(error) = appended {
        eprintln!("finish: the proposed message's {event} was not recorded in {}: {error}", journal.display());
    }
}

fn message_only(root: &Path, base: &str, message: &str) -> Option<String> {
    let cleaned = piped(root, None, &["stripspace"], Some(message))?;
    piped(root, None, &["commit-tree", &format!("{base}^{{tree}}"), "-p", base, "-F", "-"], Some(&cleaned)).map(|head| head.trim().to_owned())
}
