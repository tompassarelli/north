use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::fleet::ssh::{over_ssh, over_ssh_with, over_ssh_within};

use super::{Addressee, FleetRow, SessionRow};

const DELIVER_WAIT: Duration = Duration::from_secs(40);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FarRegistry {
    pub host: String,
    pub sessions: Vec<SessionRow>,
    pub addressees: Vec<Addressee>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Handed {
    Delivered,
    Already,
    Deferred,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HandedRow {
    pub id: String,
    pub to: String,
    pub handed: Handed,
}

pub trait FarSpools: Sync {
    fn take(&self, ssh: &str) -> NorthResult<Vec<FleetRow>>;
    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()>;
    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()>;
    fn cache(&self, ssh: &str, sessions: &[SessionRow]) -> NorthResult<()>;
    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry>;
    fn deliver(&self, ssh: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>>;
}

pub struct OverSsh;

impl FarSpools for OverSsh {
    fn take(&self, ssh: &str) -> NorthResult<Vec<FleetRow>> {
        take_with(|words| over_ssh(ssh, words))
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        drop_with(|words| over_ssh(ssh, words), ids)
    }

    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        set_aside_with(|words| over_ssh(ssh, words), ids)
    }

    fn cache(&self, ssh: &str, sessions: &[SessionRow]) -> NorthResult<()> {
        over_ssh_with(ssh, &["fleet", "spool", "cache"], &serde_json::to_vec(sessions)?).map(drop)
    }

    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry> {
        Ok(serde_json::from_slice(&over_ssh(ssh, &["fleet", "spool", "registry"])?)?)
    }

    fn deliver(&self, ssh: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(serde_json::from_slice(&over_ssh_within(ssh, &["fleet", "spool", "deliver"], Some(&serde_json::to_vec(rows)?), DELIVER_WAIT)?)?)
    }
}

pub fn take_with(run: impl Fn(&[&str]) -> NorthResult<Vec<u8>>) -> NorthResult<Vec<FleetRow>> {
    Ok(serde_json::from_slice(&run(&["fleet", "spool", "take"])?)?)
}

pub fn drop_with(run: impl Fn(&[&str]) -> NorthResult<Vec<u8>>, ids: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = ["fleet", "spool", "drop"].into_iter().chain(ids.iter().map(String::as_str)).collect();
    run(&words).map(|_| ())
}

pub fn set_aside_with(run: impl Fn(&[&str]) -> NorthResult<Vec<u8>>, ids: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = ["fleet", "spool", "set-aside"].into_iter().chain(ids.iter().map(String::as_str)).collect();
    run(&words).map(|_| ())
}
