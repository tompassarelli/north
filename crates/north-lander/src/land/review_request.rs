//! The review a hand-in's lane waits for in the landing queue (north#1632): when the land scans `review-accept` and `safe-push-keys` (north#1544, north#1610) find the lane holds no reviewer accept it needs, the hand-in queues its ticket held under the patch id the accept is keyed by and returns, and the lander makes the lane's one security read itself and takes the ticket once an accept is recorded (land/unreviewed.rs, north#1650).

use std::path::Path;

use north_base::platform::review_accept::patch_id;
use north_land_scans::review::{Review, review};
use north_landing::land::queue::Unreviewed;

use crate::land::candidate::git;

/// The review the lane in `root` at `head` over its merge-base with `base` waits for: `None` when the scans need no accept of it or refuse it for anything else, which the lander's own run of them judges; else the patch its ticket is held under, once its author is told.
pub fn held(root: &Path, base: &str, head: &str) -> Result<Option<Unreviewed>, String> {
    let Review::Awaiting(refusal) = review(root, base, head) else { return Ok(None) };
    let fork = git(root, &["merge-base", base, head])?;
    let patch = patch_id(root, &fork, head)?;
    println!("safe-push: {refusal}");
    println!("safe-push: the lane waits in the landing queue, not in this turn; the lander makes one security read of patch {patch} and takes the lane once it accepts, or refuses the lane with its answer");
    Ok(Some(Unreviewed { patch, noted: false }))
}
