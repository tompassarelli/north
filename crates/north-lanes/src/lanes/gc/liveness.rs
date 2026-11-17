use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Gc;
use super::exec::{git, real, text, verified};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Trees {
    pub(super) ok: bool,
    pub(super) branches: BTreeMap<String, String>,
}

pub(super) struct Hosts {
    pub(super) laptop: Trees,
    pub(super) nexus: Trees,
    pub(super) on_nexus: bool,
}

impl Hosts {
    pub(super) fn peer(&self) -> (&'static str, &Trees) {
        if self.on_nexus { ("Laptop", &self.laptop) } else { ("Nexus", &self.nexus) }
    }
}

pub(super) fn alive_status(session: &Value) -> bool {
    session.get("status").and_then(Value::as_str) != Some("exited")
}

impl Gc {
    pub(super) fn fleet_sessions(&mut self) -> Result<&[Value], String> {
        match self.fleet.as_deref() {
            Some(sessions) => Ok(sessions),
            None => Err("Fleet sessions unavailable: no session source".into()),
        }
    }

    pub(super) fn owner(&mut self, name: &str) -> Result<String, String> {
        let prefix = format!("/code/{name}/");
        let sessions = self.fleet_sessions()?;
        let holder = sessions.iter().find(|session| {
            session.get("cwd").and_then(Value::as_str).is_some_and(|cwd| real(Path::new(cwd)).to_string_lossy().contains(&prefix)) && alive_status(session)
        });
        let named = |key| holder.and_then(|session| session.get(key)).and_then(text).map(str::to_string);
        Ok(named("parent").or_else(|| named("name")).unwrap_or_else(|| "coordinator".into()))
    }

    pub(super) fn session_in(&mut self, path: &Path) -> Result<bool, String> {
        let lane = real(path);
        Ok(self.fleet_sessions()?.iter().any(|session| {
            session.get("cwd").and_then(Value::as_str).is_some_and(|cwd| real(Path::new(cwd)).starts_with(&lane)) && alive_status(session)
        }))
    }

    pub(super) fn local_archive_guard(&mut self, main: &Path, name: &str, branch: &str, path: Option<&Path>) -> String {
        self.archive_guard(main, name, branch, path).unwrap_or_else(|why| format!("liveness unavailable: {why}"))
    }

    fn archive_guard(&mut self, main: &Path, name: &str, branch: &str, path: Option<&Path>) -> Result<String, String> {
        if let Some(path) = path
            && self.session_in(path)?
        {
            return Ok("live session in lane".into());
        }
        if !branch.is_empty() && verified(main, &format!("refs/remotes/origin/{branch}")).is_some() {
            let (peer, trees) = self.hosts.peer();
            if !trees.ok {
                return Ok(format!("{peer} liveness unavailable"));
            }
            if let Some(why) = trees.branches.get(&format!("{name}:{branch}")).filter(|why| !why.is_empty()) {
                return Ok(format!("{peer} {why}"));
            }
        }
        Ok(String::new())
    }
}

pub(super) fn tree_is_live<'a>(trees: &'a Trees, key: &str) -> Option<&'a str> {
    trees.branches.get(key).map(String::as_str).filter(|why| !why.is_empty())
}

pub(super) fn holds_git_dir(path: &Path, common: &Path) -> bool {
    common.parent().is_some_and(|parent| real(path) == parent)
}

pub(super) fn worktree_admin(path: &Path) -> String {
    git(path, &["rev-parse", "--path-format=absolute", "--git-dir"]).out
}
