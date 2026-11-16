//! The review a hand-in's lane waits for in the landing queue (north#1632): when the land scans `review-accept` and `safe-push-keys` (north#1544, north#1610) find the lane holds no reviewer accept it needs, the hand-in queues its ticket held under the patch id the accept is keyed by and returns, and the lander takes the ticket once an accept is recorded (land/unreviewed.rs). Inside a worker domain the hand-in starts one detached `review-call HEAD` through the broker, as the session that handed in; the call is one fresh one-shot call on the patch and records the accept itself, and the broker serves it only on the host that settles the fleet. Outside a worker domain no broker answers, so the request goes to the reviewer sessions directly.

use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::platform::north_binary::land_north;
use north_base::platform::records_mirror::state_home;
use north_base::platform::review_accept::patch_id;
use north_base::platform::session_scope::{SANDBOX_MARK, SOCKETS_HIDDEN};
use north_land_scans::review::{Review, review};
use north_landing::land::queue::Unreviewed;
use north_landing::land::review_send;

use crate::land::candidate::git;

const CALLS: &str = "north/land/review-calls";

/// The review the lane in `root` at `head` over its merge-base with `base` waits for: `None` when the scans need no accept of it or refuse it for anything else, which the lander's own run of them judges; else the patch its ticket is held under, once its review is asked and its author told.
pub fn held(root: &Path, base: &str, head: &str, session: Option<&str>) -> Result<Option<Unreviewed>, String> {
    let Review::Awaiting(refusal) = review(root, base, head) else { return Ok(None) };
    let fork = git(root, &["merge-base", base, head])?;
    let patch = patch_id(root, &fork, head)?;
    let asked = started(root, head, &patch, session).unwrap_or_else(|why| format!("north started no review call: {why}"));
    println!("safe-push: {refusal}");
    println!("safe-push: the lane waits in the landing queue, not in this turn, and the lander takes it once a reviewer's accept of patch {patch} is recorded; {asked}");
    Ok(Some(Unreviewed { patch, noted: false }))
}

fn started(root: &Path, head: &str, patch: &str, session: Option<&str>) -> Result<String, String> {
    let lane = root.file_name().and_then(|lane| lane.to_str()).unwrap_or_default();
    if !sandboxed() {
        return Ok(format!("no review call of patch {patch} runs outside a worker domain; {}", review_send::request(patch, lane, head, "north starts review calls only from a worker domain")));
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
        .args(["review-call", head])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .process_group(0)
        .spawn()
        .map_err(|error| format!("north review-call {head}: {error}"))?;
    Ok(format!("north started one review call{from} of its patch {patch} detached; its verdict, or the review request sent to the reviewers when no call can run, is logged in {}, and a refusal comes back as the lander's refusal of the ticket.", log.display()))
}

fn sandboxed() -> bool {
    std::env::var_os(SANDBOX_MARK).is_some() || std::env::var_os(SOCKETS_HIDDEN).is_some()
}
