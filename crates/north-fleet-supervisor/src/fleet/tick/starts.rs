use std::io::Write;
use std::path::{Path, PathBuf};

use north_agent_starts::{REQUEST, StartRequest};

use super::Ticked;
use crate::fleet::model::{Role, SessionRecord};
use crate::platform::login::Login;

/// The one writer of start requests for the laptop's local agent sessions: the fleet tick, into the operator-owned request directory the starter reads.
#[derive(Clone, Debug)]
pub struct StartWriter {
    dir: PathBuf,
    operator_home: PathBuf,
}

impl StartWriter {
    pub fn new(dir: &Path, operator_home: &Path) -> Self {
        Self { dir: dir.to_path_buf(), operator_home: operator_home.to_path_buf() }
    }

    /// The request for `launch`, None when its record does not say it runs as a local agent login. Its asker is its parent's record: only a lead with a task asks for a start, whatever the spool file's location, the brief's text or any path the asker supplies; the fields come from the launch record alone.
    fn request(&self, launch: &SessionRecord, asker: Option<&SessionRecord>) -> Result<Option<(StartRequest, PathBuf)>, String> {
        let Some(machine) = launch.machine.as_ref().filter(|machine| matches!(machine.session_dial(), Some(Login::Local(_)))) else { return Ok(None) };
        let home = machine.session_home(&self.operator_home).ok_or_else(|| "its machine names no agent home".to_owned())?;
        let asker = asker.ok_or_else(|| "no fleet record of the session that asked for it".to_owned())?;
        if asker.role() != Role::Lead || asker.task_ref.is_none() {
            return Err(format!("{} is not a lead with a task, so it may not ask for a local agent start", asker.name));
        }
        let brief = launch.brief.as_deref().ok_or_else(|| "its record names no brief file".to_owned())?;
        let request = StartRequest { name: launch.name.clone(), account: launch.account.clone(), cwd: machine.session_path(&launch.cwd, &self.operator_home), brief: machine.session_path(brief, &self.operator_home) };
        let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        StartRequest::parse(&bytes, &home)?;
        Ok(Some((request, home)))
    }

    /// Writes the request for `launch` through a temporary name beside it and renames it into place, so the starter never reads a partial file; None when it is no local agent launch or its request still waits.
    pub fn write(&self, launch: &SessionRecord, asker: Option<&SessionRecord>) -> Result<Option<PathBuf>, String> {
        let Some((request, _)) = self.request(launch, asker)? else { return Ok(None) };
        let path = self.dir.join(format!("{}.{REQUEST}", request.name));
        if path.exists() {
            return Ok(None);
        }
        let staged = self.stage(&request)?;
        Self::place(staged, &path)?;
        Ok(Some(path))
    }

    fn stage(&self, request: &StartRequest) -> Result<tempfile::NamedTempFile, String> {
        let mut staged = tempfile::Builder::new().prefix(".").suffix(".tmp").tempfile_in(&self.dir).map_err(|error| format!("cannot stage a request in {}: {error}", self.dir.display()))?;
        staged.write_all(request.line().as_bytes()).map_err(|error| format!("cannot write the request: {error}"))?;
        Ok(staged)
    }

    fn place(staged: tempfile::NamedTempFile, path: &Path) -> Result<(), String> {
        staged.persist(path).map(drop).map_err(|error| format!("cannot place {}: {}", path.display(), error.error))
    }
}

pub(super) fn write_starts(writer: &StartWriter, records: &[SessionRecord], ticked: &mut Ticked) {
    for launch in records.iter().filter(|record| record.host_pid.is_none() && !record.ended()) {
        let asker = launch.parent.as_deref().and_then(|parent| records.iter().find(|record| record.name == parent));
        match writer.write(launch, asker) {
            Ok(Some(_)) => ticked.started.push(launch.name.clone()),
            Ok(None) => {}
            Err(why) => ticked.unstarted.push((launch.name.clone(), why)),
        }
    }
}

#[cfg(test)]
#[path = "starts_tests.rs"]
mod tests;
