use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

use super::super::gates::GateRequest;
use super::super::model::Delivery;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RowKind {
    #[serde(rename = "MESSAGE")]
    Message,
    #[serde(rename = "GATE")]
    Gate,
    #[serde(rename = "ANSWER")]
    Answer,
    #[serde(rename = "JOB-START")]
    JobStart,
    #[serde(rename = "JOB-BEAT")]
    JobBeat,
    #[serde(rename = "JOB-EXIT")]
    JobExit,
}

impl RowKind {
    pub const ALL: [RowKind; 6] = [Self::Message, Self::Gate, Self::Answer, Self::JobStart, Self::JobBeat, Self::JobExit];

    pub fn label(self) -> &'static str {
        match self {
            Self::Message => "MESSAGE",
            Self::Gate => "GATE",
            Self::Answer => "ANSWER",
            Self::JobStart => "JOB-START",
            Self::JobBeat => "JOB-BEAT",
            Self::JobExit => "JOB-EXIT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JobPayload {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FleetRow {
    pub id: String,
    pub kind: RowKind,
    #[serde(rename = "to_node", alias = "to")]
    pub to: String,
    pub from: String,
    #[serde(default)]
    pub urgent: bool,
    pub sent_at: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Box<GateRequest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobPayload>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Rows {
    Many(Vec<FleetRow>),
    One(Box<FleetRow>),
}

impl FleetRow {
    pub fn delivery(&self) -> Delivery {
        if self.urgent { Delivery::Now } else { Delivery::Queued }
    }

    pub fn prompt_id(&self) -> String {
        let mut bytes: [u8; 16] = Sha256::digest(self.id.as_bytes())[..16].try_into().unwrap_or_default();
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
    }

    pub fn signer(&self) -> String {
        self.sender.as_ref().map_or_else(|| self.from.clone(), |sender| format!("{sender}@{}", self.from))
    }

    pub fn check_id(id: &str) -> NorthResult<()> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(NorthError::Usage(format!("not a row id: {id}")));
        }
        Ok(())
    }

    pub fn validate(&self) -> NorthResult<()> {
        Self::check_id(&self.id)?;
        if self.to.trim().is_empty() || self.from.trim().is_empty() {
            return Err(NorthError::Usage(format!("row {} needs a sender and a recipient", self.id)));
        }
        Ok(())
    }

    pub fn parse(input: &[u8]) -> NorthResult<Vec<Self>> {
        let rows = match serde_json::from_slice::<Rows>(input)? {
            Rows::Many(rows) => rows,
            Rows::One(row) => vec![*row],
        };
        rows.iter().try_for_each(Self::validate)?;
        Ok(rows)
    }
}
