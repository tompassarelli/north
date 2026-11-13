use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use north_base::platform::finish_record::{FinishRecords, Finished};
use north_base::platform::review_accept::Accepts;

use crate::{Range, git, holdlist, review_accept};

const SELF: &str = "safe-push-keys";
const SETTINGS: &str = ".safe-push";

type Keys = BTreeMap<String, Vec<String>>;

fn keys(directory: &Path, revision: &str) -> Keys {
    let mut keys = Keys::new();
    let listed = git::text(directory, &["config", "--blob", &format!("{revision}:{SETTINGS}"), "--list", "-z"]).unwrap_or_default();
    for (key, value) in listed.split('\0').filter_map(|entry| entry.split_once('\n')) {
        keys.entry(key.to_owned()).or_default().extend(value.lines().map(str::to_owned));
    }
    keys
}

/// The keys of `.safe-push` whose lines differ between the landing `base` and the lane's `head`.
fn changed(directory: &Path, base: &str, head: &str) -> Vec<String> {
    let (before, after) = (keys(directory, base), keys(directory, head));
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    names.into_iter().filter(|name| before.get(*name) != after.get(*name)).cloned().collect()
}

/// `Err` is a lane that changes a key of `.safe-push` and holds no review accept of its patch id from a reviewer session that did not author it; a reviewer a commit message names grants nothing, since only a recorded accept counts.
pub fn check(directory: &Path, base: &str, head: &str, accepts: &Accepts, finished: &[Finished]) -> Result<(), String> {
    let fork = git::merge_base(directory, base, head);
    let changed = changed(directory, &fork, head);
    if changed.is_empty() {
        return Ok(());
    }
    let (patch, accepted) = review_accept::reviewers(directory, &fork, head, accepts, finished)?;
    if !accepted.is_empty() {
        return Ok(());
    }
    Err(format!("the lane changes {} of {SETTINGS}, which the landing judges by main's lines, and holds no review accept of its patch {patch} from a reviewer session that did not author the lane; a reviewer's `north runtime accept {head}` records one", changed.join(", ")))
}

pub fn scan(directory: &Path, range: &str, state: &Path, accepts: &Accepts, finished: &FinishRecords, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let outcome = check(directory, base, head, accepts, &finished.all());
    let enforce = holdlist::enforcing(directory, SELF, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        holdlist::record(state, SELF, head, outcome.as_ref().err().map(String::as_str));
    }
    match outcome {
        Ok(()) => 0,
        Err(refusal) => {
            let verb = if enforce { "refused" } else { "would refuse (shadow)" };
            let _ = writeln!(out, "{SELF}: {verb}: {refusal}");
            u8::from(enforce)
        }
    }
}

#[cfg(test)]
#[path = "safe_push_keys_tests.rs"]
mod tests;
