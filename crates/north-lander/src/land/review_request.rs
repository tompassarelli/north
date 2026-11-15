//! The review request a hand-in sends itself when the land scan `review-accept` or `safe-push-keys` refuses its lane before queueing for missing accepts (north#1544): the refusal is decided inside a build host's candidate jail, which sends no fleet message, so the hand-in on the host that handed in reads the refusal text and asks each session `review_request_sessions` of the harness/defaults.toml names, once per patch id, to review the lane and accept it by its head's full sha, once `north runtime review-fetch` has published the head into the trusted mirror of the host that settles the fleet as refs/review/LANE (north#1610); a head the mirror does not hold sends no request.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::north_binary::land_north;
use north_base::platform::records_mirror::state_home;
use north_base::platform::review_accept::{Accepts, patch_id};

use super::enqueue::git;

const SCANS: [&str; 2] = ["review-accept: refused: ", "safe-push-keys: refused: "];
const SESSIONS: &str = "review_request_sessions";
const SENT: &str = "north/land/review-requests";

/// What the author is told when `refusal` is a scan's refusal for missing accepts of the lane in `root` at `head` over its merge-base with `base`: which reviewers were asked, or that the request for this patch is already out; `None` for any other refusal.
pub fn ask(root: &Path, base: &str, head: &str, session: Option<&str>, refusal: &str) -> Option<String> {
    SCANS.iter().any(|scan| refusal.contains(scan)).then(|| asked(root, base, head, session).unwrap_or_else(|why| format!("north sent no review request: {why}")))
}

/// Prints, after `verb`, what [`ask`] tells the author of a refused finish call's check.
pub fn tell(verb: &str, root: &Path, (base, head): (&str, &str), session: Option<&str>, refusal: &str) {
    ask(root, base, head, session, refusal).into_iter().for_each(|asked| eprintln!("{verb}: {asked}"));
}

fn asked(root: &Path, base: &str, head: &str, session: Option<&str>) -> Result<String, String> {
    let head = git(root, &["rev-parse", "--verify", &format!("{head}^{{commit}}")])?;
    let fork = git(root, &["merge-base", base, &head])?;
    let patch = patch_id(root, &fork, &head)?;
    let record = state_home().join(SENT).join(&patch);
    let earlier = std::fs::read_to_string(&record).unwrap_or_default();
    let accepted = Accepts::here().sessions(&patch).unwrap_or_default();
    let due: Vec<String> = reviewers().into_iter().filter(|reviewer| !accepted.contains(reviewer) && !earlier.lines().any(|asked| asked == reviewer)).collect();
    if due.is_empty() {
        let out = earlier.lines().collect::<Vec<_>>().join(" and ");
        return Ok(format!("the review request for its patch {patch} is already out to {}, so none was sent again", if out.is_empty() { "every reviewer that has not accepted it" } else { &out }));
    }
    let lane = root.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let place = publish(root, lane, &head)?;
    let from = session.map_or_else(String::new, |session| format!(" for {session}"));
    let (host, path) = (north_base::platform::host::name(), root.display());
    let text = format!(
        "Review request{from}, sent by north because a land scan refused the hand-in for missing review accepts: lane {lane} at {path} on host {host}, head {head}, base {fork}, patch {patch} (`git patch-id --stable` of base..head over the whole lane). The trusted mirror holds the head as refs/review/{lane}, fetched from {place}; `north ln {lane}` reads it, and `git diff {fork} {head}` in the main checkout once fetched. Once it holds, `north runtime accept {head}` records your accept."
    );
    let sent: Vec<&str> = due.iter().map(String::as_str).filter(|reviewer| send(reviewer, &text)).collect();
    if sent.is_empty() {
        return Err(format!("`north fleet send` reached none of {}", due.join(", ")));
    }
    if let Err(error) = keep(&record, &sent) {
        eprintln!("north: the review request for {patch} was not recorded in {}: {error}", record.display());
    }
    Ok(format!("north asked {} to review the lane and accept its patch {patch}; a second hand-in of this patch sends no second request to them", sent.join(" and ")))
}

fn reviewers() -> Vec<String> {
    document(DEFAULTS)[SESSIONS]["value"].as_array().map(|names| names.iter().filter_map(|name| name.as_str()).map(str::to_owned).collect()).unwrap_or_default()
}

fn send(reviewer: &str, text: &str) -> bool {
    Command::new(land_north()).args(["fleet", "send", reviewer, text]).stdin(Stdio::null()).stdout(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn keep(record: &Path, sent: &[&str]) -> std::io::Result<()> {
    record.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    let mut file = OpenOptions::new().create(true).append(true).open(record)?;
    sent.iter().try_for_each(|reviewer| writeln!(file, "{reviewer}"))
}

/// Publishes the lane `lane` in `root` into the trusted mirror of the host that settles the fleet with `north runtime review-fetch`, so a reviewer reads it with no patch sent by message; where it was fetched from, or why the mirror does not hold `head` as refs/review/LANE, in which case no review request goes out.
fn publish(root: &Path, lane: &str, head: &str) -> Result<String, String> {
    let output = Command::new(land_north()).args(["runtime", "review-fetch", lane]).current_dir(root).stdin(Stdio::null()).output().map_err(|error| format!("north runtime review-fetch {lane}: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let held = format!("refs/review/{lane} is {head} on ");
    text.lines().find_map(|line| line.split_once(&held).map(|(_, place)| place.trim().to_owned())).ok_or_else(|| {
        format!("the trusted mirror does not hold head {head} as refs/review/{lane}, so no reviewer could read the lane: {}{}", text.trim(), String::from_utf8_lossy(&output.stderr).trim())
    })
}
