use std::fs;

use crate::error::NorthResult;

use super::super::claims;
use super::super::host::Host;
use super::super::model::Ending;
use super::super::records::{ARCHIVE, write_record};
use super::super::runtime::now_ms;
use super::super::unaccepted::Unaccepted;
use super::{Fleet, session_running};

impl Fleet {
    pub fn stop(&self, name: &str) -> NorthResult<()> {
        let mut record = self.record(name)?;
        let _start = self.start_lock(name)?;
        let host = Host::new(record.host_dir.clone());
        if session_running(&record) {
            let runtime = self.runtime(&record)?;
            let transcript = self.recent(runtime.as_ref(), &record)?;
            let _ = host.send(&runtime.close(&transcript));
        }
        let terminate = !self.host_in_use(&record)?;
        let (was_stopped, was_ended) = (record.stopped, record.ended);
        record.stopped = true;
        record.ended = Some(Ending::Stopped);
        write_record(&self.root.join(name), &record)?;
        if terminate && let Err(error) = host.terminate() {
            record.stopped = was_stopped;
            record.ended = was_ended;
            write_record(&self.root.join(name), &record)?;
            return Err(error);
        }
        if terminate {
            Unaccepted::discard(&record.host_dir);
        }
        claims::release_for(self.claims.as_ref(), &record);
        Ok(())
    }

    pub fn forget(&self, name: &str) -> NorthResult<()> {
        self.stop(name)?;
        let archive = self.root.join(ARCHIVE);
        fs::create_dir_all(&archive)?;
        fs::rename(self.root.join(name), archive.join(format!("{name}-{}", now_ms())))?;
        Ok(())
    }
}
