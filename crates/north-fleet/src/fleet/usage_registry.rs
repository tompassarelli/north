use std::fs;
use std::path::PathBuf;

use super::records::read_record;
use crate::usage::decide::world::{
    EstimateSource, Expected, Label, Name, Owner, Session, Standing, Usd, Workstream,
};
use crate::usage::shell::{Registry, family};

pub struct FleetRecords {
    pub root: PathBuf,
}

impl Registry for FleetRecords {
    fn sessions(&self) -> Result<Vec<Session>, String> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("{}: {error}", self.root.display())),
        };
        let mut sessions: Vec<Session> = entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| read_record(&entry.path()).ok())
            .filter(|record| {
                record.provider.label() == "claude"
                    && !record.stopped
                    && record.host_pid.is_some_and(|pid| {
                        std::path::Path::new("/proc").join(pid.to_string()).exists()
                    })
            })
            .filter_map(|record| {
                Some(Session {
                    standing: if record.name.starts_with("lead-") { Standing::Lead } else { Standing::Worker },
                    name: Name(record.name),
                    owner: Owner::Fleet,
                    account: Label(record.account),
                    family: family(record.model.as_deref()?)?,
                    workstream: record.task_ref.clone().map(Workstream),
                    box_id: None,
                    spent: None,
                    active_minutes: 0.0,
                    expected: Expected {
                        p50: Usd(0.0),
                        p90: Usd(0.0),
                        minutes: 0.0,
                        source: EstimateSource::None,
                    },
                    parked_since: None,
                })
            })
            .collect();
        sessions.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(sessions)
    }
}
