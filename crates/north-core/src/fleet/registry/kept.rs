use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::NorthResult;
use crate::fleet::detached;
use crate::fleet::invariant::{LastTurn, Mark};
use crate::fleet::model::{EntryKind, SessionRecord};
use crate::fleet::store::WaitKey;
use crate::fleet::transcript::{session_stream_path, stream_path};

use super::Fleet;

const MARK: &str = "invariant.json";
const UNSETTLED: &str = ".unsettled-waits.json";

impl Fleet {
    pub fn last_turn(&self, record: &SessionRecord) -> Option<LastTurn> {
        let follower = self.follow_recent(&record.name).ok()?;
        let transcript = follower.transcript();
        let turn = transcript.turns.last()?;
        let reply = transcript.entries.iter().rfind(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.clone()).unwrap_or_default();
        let id = fs::metadata(session_stream_path(&record.host_dir, &record.name)).ok()?.len();
        let ended_ms = u64::try_from(written(record)?.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
        Some(LastTurn { reply, id, ended_ms, exit: turn.exit })
    }

    pub fn cli_is_working(&self, record: &SessionRecord) -> bool {
        record.machine.is_none() && record.host_pid.is_some_and(|host| detached::cli_work(Path::new("/proc"), Path::new("/sys/fs/cgroup"), host) > 0)
    }

    pub fn active_within(&self, record: &SessionRecord, window: Duration) -> bool {
        written(record).is_some_and(|at| SystemTime::now().duration_since(at).map_or(true, |age| age < window))
    }

    pub fn invariant_mark(&self, name: &str) -> Mark {
        self.session_dir(name).ok().and_then(|dir| fs::read(dir.join(MARK)).ok()).and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn set_invariant_mark(&self, name: &str, mark: Mark) -> NorthResult<()> {
        let dir = self.session_dir(name)?;
        let scratch = dir.join(format!(".{MARK}.tmp"));
        fs::write(&scratch, serde_json::to_vec(&mark)?)?;
        fs::rename(scratch, dir.join(MARK))?;
        Ok(())
    }

    pub fn unsettled_waits(&self) -> Vec<WaitKey> {
        fs::read(self.root.join(UNSETTLED)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn set_unsettled_waits(&self, waits: &[WaitKey]) -> NorthResult<()> {
        let file = self.root.join(UNSETTLED);
        if waits.is_empty() {
            return match fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        let scratch = self.root.join(format!("{UNSETTLED}.tmp"));
        fs::write(&scratch, serde_json::to_vec(waits)?)?;
        fs::rename(scratch, file)?;
        Ok(())
    }

    pub fn gated_sessions(&self, now_ms: u64) -> NorthResult<Vec<String>> {
        Ok(self.ledger.gates(now_ms)?.into_iter().filter(|gate| gate.is_open()).map(|gate| gate.request.session).collect())
    }
}

fn written(record: &SessionRecord) -> Option<SystemTime> {
    fs::metadata(stream_path(&record.host_dir)).and_then(|meta| meta.modified()).ok()
}
