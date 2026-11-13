//! A reviewer's accept of a patch (north#1036): a record in the verdict store, which no worker domain writes, saying that the fleet session whose cgroup scope the broker read accepts the patch `git patch-id --stable` names. The patch id of base..head survives the lander's rebase, so the accepts of the lane the reviewers read count for the commit the lander makes of it.
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::platform::finish_record::verdict_store;
use crate::platform::{git, process};

const DIR: &str = "accepts";
const PATCH_ID_LIMIT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accept {
    pub patch_id: String,
    pub session: String,
    pub at: u64,
}

pub struct Accepts(pub PathBuf);

pub fn is_patch_id(word: &str) -> bool {
    matches!(word.len(), 40 | 64) && word.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_session(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// `git patch-id --stable` of the change from `from` to `to` in `directory`.
pub fn patch_id(directory: &Path, from: &str, to: &str) -> Result<String, String> {
    let diff = git::stdout(directory, &["diff", "--no-ext-diff", "--no-renames", "--binary", from, to], |output| format!("git diff {from} {to}: {}", String::from_utf8_lossy(&output.stderr).trim()))?;
    let output = process::run_with_input_within("git", &["patch-id", "--stable"], Some(&diff), PATCH_ID_LIMIT).filter(|output| output.status.success()).ok_or_else(|| format!("git patch-id --stable of {from}..{to} did not answer"))?;
    String::from_utf8_lossy(&output.stdout).split_whitespace().next().filter(|id| is_patch_id(id)).map(str::to_owned).ok_or_else(|| format!("{from}..{to} changes nothing, so it has no patch id"))
}

impl Accepts {
    pub fn here() -> Self {
        let variables: Vec<(String, String)> = ["NORTH_VERDICT_CACHE_DIR", "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
        Self::of(&variables, &PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))
    }

    pub fn of(environment: &[(String, String)], home: &Path) -> Self {
        Self(verdict_store(environment, home).join(DIR))
    }

    fn file(patch_id: &str, session: &str) -> String {
        format!("{patch_id}.{session}.json")
    }

    pub fn record(&self, accept: &Accept) -> Result<(), String> {
        if !is_patch_id(&accept.patch_id) || !is_session(&accept.session) {
            return Err(format!("{:?} by {:?} is no accept of a patch id by a fleet session", accept.patch_id, accept.session));
        }
        let failure = |error: std::io::Error| format!("the accept of {} in {}: {error}", accept.patch_id, self.0.display());
        fs::create_dir_all(&self.0).map_err(failure)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0).map_err(failure)?;
        fs::write(pending.path(), serde_json::to_string(accept).map_err(|error| error.to_string())?).map_err(failure)?;
        pending.persist(self.0.join(Self::file(&accept.patch_id, &accept.session))).map(|_| ()).map_err(|error| failure(error.error))
    }

    /// The sessions that accepted `patch_id`; a store that exists and cannot be read, or an entry of it that is not the accept its name says, is an error, never fewer accepts.
    pub fn sessions(&self, patch_id: &str) -> Result<BTreeSet<String>, String> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
            Err(error) => return Err(format!("the accepts in {} cannot be listed: {error}", self.0.display())),
        };
        let mut sessions = BTreeSet::new();
        for entry in entries {
            let path = entry.map_err(|error| format!("an entry of {} cannot be read: {error}", self.0.display()))?.path();
            let text = fs::read_to_string(&path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
            let accept = serde_json::from_str::<Accept>(&text).map_err(|error| format!("{} is no accept: {error}", path.display()))?;
            if path.file_name().is_none_or(|name| *name != *Self::file(&accept.patch_id, &accept.session)) {
                return Err(format!("{} holds the accept of {} by {}, which is not the accept its name says", path.display(), accept.patch_id, accept.session));
            }
            if accept.patch_id == patch_id {
                sessions.insert(accept.session);
            }
        }
        Ok(sessions)
    }
}
