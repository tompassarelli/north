use std::path::PathBuf;

use crate::error::NorthResult;

use super::Fleet;
use super::super::model::{Role, SessionRecord};
use super::super::runtime::PROXY;
use super::super::store::{Addressee, Claim, FleetRow, RowKind};

/// The session a takeover ended, and where its directory now lies.
pub(super) struct Predecessor {
    record: SessionRecord,
    pub(super) archived: PathBuf,
}

impl Predecessor {
    /// The successor takes every message to the name from where the predecessor's line began, so a report sent before the takeover that the old session never took still arrives.
    pub(super) fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub(super) fn addressed_since(&self) -> u64 {
        self.record.addressed_since()
    }
}

impl Fleet {
    /// Ends and archives the session holding `name` when this launch takes the name over; None when no session holds it.
    pub(super) fn retire_for(&self, name: &str, claim: Claim) -> NorthResult<Option<Predecessor>> {
        if claim != Claim::TakeOver || !self.session_dir(name)?.exists() {
            return Ok(None);
        }
        let record = self.record(name)?;
        let archived = self.archive(name)?;
        eprintln!("north fleet: {name} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", record.conversation.as_deref().unwrap_or("none"), archived.display());
        Ok(Some(Predecessor { record, archived }))
    }

    pub fn fleet_proxy(&self) -> NorthResult<Option<SessionRecord>> {
        Ok(self.sessions()?.into_iter().find(|record| record.name == PROXY && record.role == Some(Role::Proxy)))
    }

    pub fn earlier_proxies(&self) -> NorthResult<Vec<SessionRecord>> {
        let earlier = |record: &SessionRecord| match record.role {
            None => record.name == PROXY || record.name.starts_with("proxy-"),
            Some(role) => role == Role::Proxy && record.name != PROXY,
        };
        Ok(self.sessions()?.into_iter().filter(earlier).collect())
    }

    pub fn retire_proxy(&self, name: &str) -> NorthResult<PathBuf> {
        self.archive(name)
    }

    pub fn addressee(&self, record: &SessionRecord, floor: u64) -> NorthResult<Addressee> {
        let taken = self.delivered(&record.name)?.into_iter().filter(|id| sent_at(id).is_none_or(|ms| ms >= floor)).collect();
        Ok(Addressee { name: record.name.clone(), stopped: record.stopped, control_since_ms: Some(record.created_ms.max(floor)), since_ms: record.addressed_since().max(floor), taken })
    }
}

pub fn backlog_line(retired: &[String], rows: &[FleetRow], now_ms: u64) -> String {
    let messages: Vec<&FleetRow> = rows.iter().filter(|row| row.kind == RowKind::Message).collect();
    let names = retired.join(" and ");
    let Some(oldest) = messages.iter().map(|row| row.sent_at).min() else {
        return format!("[north: north proxy archived {names}; no messages to them were waiting]");
    };
    let mut senders: Vec<String> = messages.iter().map(|row| row.sender.prefix().unwrap_or_else(|| "Tom".to_owned())).collect();
    senders.sort();
    senders.dedup();
    let count = messages.len();
    format!(
        "[north: north proxy archived {names}; {count} message{} to them were not replayed, from {}, the oldest {} old]",
        if count == 1 { "" } else { "s" },
        senders.join(", "),
        age(now_ms.saturating_sub(oldest))
    )
}

fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

fn sent_at(id: &str) -> Option<u64> {
    id.split('-').next()?.parse().ok()
}
