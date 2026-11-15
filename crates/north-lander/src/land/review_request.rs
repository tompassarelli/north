//! The review call a hand-in starts itself when the land scan `review-accept` or `safe-push-keys` refuses its lane before queueing for missing accepts (north#1544, north#1610): the refusal is decided inside a build host's candidate jail, so the hand-in on the host that handed in reads the refusal text. Inside a worker domain it starts one detached `review-call HEAD` through the broker, as the session that handed in; the call is one fresh one-shot call on the patch and records the accept itself, and the broker serves it only on the host that settles the fleet. Outside a worker domain no broker answers, so the request goes to the reviewer sessions directly.

use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::platform::north_binary::land_north;
use north_base::platform::records_mirror::state_home;
use north_base::platform::review_accept::patch_id;
use north_base::platform::session_scope::{SANDBOX_MARK, SOCKETS_HIDDEN};
use north_landing::land::review_send;

use super::enqueue::git;

const SCANS: [&str; 2] = ["review-accept: refused: ", "safe-push-keys: refused: "];
const CALLS: &str = "north/land/review-calls";

/// What the author is told when `refusal` is a scan's refusal for missing accepts of the lane in `root` at `head` over its merge-base with `base`: which review call started and where its verdict is logged, or why none did; `None` for any other refusal.
pub fn ask(root: &Path, base: &str, head: &str, session: Option<&str>, refusal: &str) -> Option<String> {
    SCANS.iter().any(|scan| refusal.contains(scan)).then(|| started(root, base, head, session).unwrap_or_else(|why| format!("north started no review call: {why}")))
}

/// Prints, after `verb`, what [`ask`] tells the author of a refused finish call's check.
pub fn tell(verb: &str, root: &Path, (base, head): (&str, &str), session: Option<&str>, refusal: &str) {
    ask(root, base, head, session, refusal).into_iter().for_each(|asked| eprintln!("{verb}: {asked}"));
}

fn started(root: &Path, base: &str, head: &str, session: Option<&str>) -> Result<String, String> {
    let head = git(root, &["rev-parse", "--verify", &format!("{head}^{{commit}}")])?;
    let fork = git(root, &["merge-base", base, &head])?;
    let patch = patch_id(root, &fork, &head)?;
    let lane = root.file_name().and_then(|lane| lane.to_str()).unwrap_or_default();
    if !sandboxed() {
        return Ok(format!("no review call of patch {patch} runs outside a worker domain; {}", review_send::request(&patch, lane, &head, "north starts review calls only from a worker domain")));
    }
    let from = session.map_or_else(String::new, |session| format!(" for {session}"));
    let log = state_home().join(CALLS).join(format!("{patch}.log"));
    let failure = |error: std::io::Error| format!("the log {}: {error}", log.display());
    log.parent().map_or(Ok(()), fs::create_dir_all).map_err(failure)?;
    let output = File::create(&log).map_err(failure)?;
    let errors = output.try_clone().map_err(failure)?;
    Command::new(land_north())
        .args(["--worker-sandbox", "--outside", "--"])
        .arg(land_north())
        .args(["review-call", &head])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .process_group(0)
        .spawn()
        .map_err(|error| format!("north review-call {head}: {error}"))?;
    Ok(format!("north started one review call{from} of its patch {patch} detached; its verdict, or the review request sent to the reviewers when no call can run, is logged in {}. Hand the lane in again once an ACCEPT is recorded.", log.display()))
}


fn sandboxed() -> bool {
    std::env::var_os(SANDBOX_MARK).is_some() || std::env::var_os(SOCKETS_HIDDEN).is_some()
}
