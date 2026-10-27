use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub name: String,
    pub provider: String,
    pub account: String,
    pub status: String,
    pub parent: Option<String>,
    pub cwd: String,
    pub placement: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GateView {
    pub id: i64,
    pub session: String,
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub host: String,
    pub sessions: Vec<SessionView>,
    pub gates: Vec<GateView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub ssh: String,
    pub at_ms: u64,
    pub snapshot: Snapshot,
}

impl Seen {
    pub fn qualified(&self, gate: &GateView) -> String {
        qualified(&self.snapshot.host, gate.id)
    }
}

pub fn qualified(host: &str, id: i64) -> String {
    format!("{host}:{id}")
}

pub fn split_qualified(label: &str) -> Option<(&str, i64)> {
    let (host, id) = label.rsplit_once(':')?;
    (!host.is_empty()).then_some(())?;
    Some((host, id.parse().ok()?))
}

