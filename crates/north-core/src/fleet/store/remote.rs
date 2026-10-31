use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::fleet::runtime::binary;
use crate::fleet::ssh::{SSH_WAIT, over_ssh_via};

use super::{Addressee, FleetRow, SessionRow, Spooled};

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pull {
    pub rows: Vec<FleetRow>,
    pub undecodable: Vec<Undecodable>,
    pub unknown_kind: Vec<UnknownKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Undecodable {
    pub id: String,
    pub why: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownKind {
    pub id: String,
    pub kind: String,
}

impl From<Vec<FleetRow>> for Pull {
    fn from(rows: Vec<FleetRow>) -> Self {
        Self { rows, ..Self::default() }
    }
}

pub trait FarSpools: Sync {
    fn take(&self, ssh: &str) -> NorthResult<Pull>;
    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()>;
    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()>;
    fn cache(&self, ssh: &str, sessions: &[SessionRow]) -> NorthResult<()>;
    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry>;
    fn deliver(&self, ssh: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>>;
}

pub struct OverSsh {
    ssh: PathBuf,
}

impl Default for OverSsh {
    fn default() -> Self {
        Self { ssh: binary("ssh") }
    }
}

impl OverSsh {
    pub fn through(ssh: PathBuf) -> Self {
        Self { ssh }
    }

    fn run(&self, host: &str, words: &[&str], input: &[u8]) -> NorthResult<Vec<u8>> {
        over_ssh_via(&self.ssh, host, words, Some(input), SSH_WAIT)
    }
}

impl FarSpools for OverSsh {
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        take_with(|words, input| self.run(ssh, words, input))
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        drop_with(|words, input| self.run(ssh, words, input), ids)
    }

    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        set_aside_with(|words, input| self.run(ssh, words, input), ids)
    }

    fn cache(&self, ssh: &str, sessions: &[SessionRow]) -> NorthResult<()> {
        self.run(ssh, &["fleet", "spool", "cache"], &serde_json::to_vec(sessions)?).map(drop)
    }

    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry> {
        Ok(serde_json::from_slice(&self.run(ssh, &["fleet", "spool", "registry"], &[])?)?)
    }

    fn deliver(&self, ssh: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(serde_json::from_slice(&over_ssh_via(&self.ssh, ssh, &["fleet", "spool", "deliver"], Some(&serde_json::to_vec(rows)?), DELIVER_WAIT)?)?)
    }
}

pub fn take_with(run: impl Fn(&[&str], &[u8]) -> NorthResult<Vec<u8>>) -> NorthResult<Pull> {
    let spooled: Vec<Spooled> = serde_json::from_slice(&run(&["fleet", "spool", "take"], &[])?)?;
    let mut pull = Pull::default();
    for Spooled { id, row } in spooled {
        if let Some(kind) = FleetRow::unknown_kind(&row) {
            pull.unknown_kind.push(UnknownKind { id, kind: kind.to_owned() });
            continue;
        }
        match serde_json::from_value::<FleetRow>(row) {
            Ok(row) => pull.rows.push(row),
            Err(error) => pull.undecodable.push(Undecodable { id, why: format!("this build cannot read it: {error}") }),
        }
    }
    Ok(pull)
}

pub fn drop_with(run: impl Fn(&[&str], &[u8]) -> NorthResult<Vec<u8>>, ids: &[String]) -> NorthResult<()> {
    run(&["fleet", "spool", "drop"], ids.join("\n").as_bytes()).map(|_| ())
}

pub fn set_aside_with(run: impl Fn(&[&str], &[u8]) -> NorthResult<Vec<u8>>, ids: &[String]) -> NorthResult<()> {
    run(&["fleet", "spool", "set-aside"], ids.join("\n").as_bytes()).map(|_| ())
}
