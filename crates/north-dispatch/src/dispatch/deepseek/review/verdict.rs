//! `north fleet dispatch verdict ...`: the verdict a reviewer records, and the rejected lane it removes.

use super::brief::CLASSES;
use super::lane::{git, north, Worktree};
use super::super::sampling::{self, Kind, Verdict};
use super::super::super::record;
use crate::pacing::placement;
use std::path::Path;

/// `north fleet dispatch verdict --session NAME --category C --sha SHA --lane LANE [--effort medium|high] clean | defect CLASS`: a reviewer's
/// verdict on a DeepSeek commit, run from the main checkout the reviewer launched in.
pub fn verdict(arguments: &[String]) -> u8 {
    let usage = || {
        eprintln!("usage: north fleet dispatch verdict --session NAME --category C --sha SHA --lane LANE [--effort medium|high] clean | defect CLASS ({})", CLASSES.map(|(class, _)| class).join(", "));
        2
    };
    let [session_flag, session, category_flag, category, sha_flag, sha, lane_flag, lane, rest @ ..] = arguments else { return usage() };
    if (session_flag.as_str(), category_flag.as_str(), sha_flag.as_str(), lane_flag.as_str()) != ("--session", "--category", "--sha", "--lane") {
        return usage();
    }
    let (effort, rest) = match rest {
        [flag, effort, rest @ ..] if flag == "--effort" && sampling::REVIEWER_EFFORTS.contains(&effort.as_str()) => (Some(effort.clone()), rest),
        _ => (None, rest),
    };
    let (kind, class) = match rest {
        [clean] if clean == "clean" => (Kind::Clean, None),
        [defect, class] if defect == "defect" && CLASSES.iter().any(|(known, _)| known == class) => (Kind::Defect, Some(class.clone())),
        _ => return usage(),
    };
    let rejected = class.clone().filter(|_| kind == Kind::Defect);
    match sampling::append(&record::directory(), &Verdict { at: placement::now(), session: session.clone(), category: category.clone(), sha: sha.clone(), kind, class, effort }) {
        Ok(()) => {
            if let Some(class) = rejected {
                reject_lane(session, sha, &class, Path::new(lane));
            }
            0
        }
        Err(why) => {
            eprintln!("north fleet dispatch verdict: {why}");
            1
        }
    }
}

/// The top of the worktree `lane` names, when it is a lane under a `worktrees` directory, else nothing.
pub(super) fn rejected_lane(lane: &Path) -> Option<String> {
    let top = git(&Worktree::here(lane.to_owned()), &["rev-parse", "--show-toplevel"]).ok()?;
    Path::new(&top).parent().is_some_and(|parent| parent.ends_with("worktrees")).then_some(top)
}

fn reject_lane(session: &str, sha: &str, class: &str, lane: &Path) {
    let Some(top) = rejected_lane(lane) else { return };
    match north(&["lane", "remove", "--rejected", &format!("{class} (review of {session} at {sha})"), &top]) {
        Ok(line) => println!("{line}"),
        Err(why) => eprintln!("north fleet dispatch verdict: the rejected lane {top} stays: {why}"),
    }
}
