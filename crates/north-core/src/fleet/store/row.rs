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
    #[serde(rename = "GATE-ANSWER")]
    GateAnswer,
    #[serde(rename = "JOB-START")]
    JobStart,
    #[serde(rename = "JOB-BEAT")]
    JobBeat,
    #[serde(rename = "JOB-EXIT")]
    JobExit,
    #[serde(rename = "CONTROL")]
    Control,
    #[serde(rename = "WAIT")]
    Wait,
}

impl RowKind {
    pub const ALL: [RowKind; 8] = [Self::Message, Self::Gate, Self::GateAnswer, Self::JobStart, Self::JobBeat, Self::JobExit, Self::Control, Self::Wait];
    pub const DELIVERED: [RowKind; 6] = [Self::Message, Self::Gate, Self::GateAnswer, Self::JobExit, Self::Control, Self::Wait];

    pub fn label(self) -> &'static str {
        match self {
            Self::Message => "MESSAGE",
            Self::Gate => "GATE",
            Self::GateAnswer => "GATE-ANSWER",
            Self::JobStart => "JOB-START",
            Self::JobBeat => "JOB-BEAT",
            Self::JobExit => "JOB-EXIT",
            Self::Control => "CONTROL",
            Self::Wait => "WAIT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnswerPayload {
    pub gate: i64,
    pub number: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlVerb {
    Forget,
    Stop,
    Interrupt,
}

impl ControlVerb {
    pub fn label(self) -> &'static str {
        match self {
            Self::Forget => "forget",
            Self::Stop => "stop",
            Self::Interrupt => "interrupt",
        }
    }

    pub fn parse(word: &str) -> NorthResult<Self> {
        [Self::Forget, Self::Stop, Self::Interrupt]
            .into_iter()
            .find(|verb| verb.label() == word)
            .ok_or_else(|| NorthError::Usage(format!("not a control verb: {word}")))
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
    pub answer: Option<AnswerPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Rows {
    Many(Vec<FleetRow>),
    One(Box<FleetRow>),
}

impl FleetRow {
    pub fn unknown_kind(value: &serde_json::Value) -> Option<&str> {
        value.get("id").and_then(serde_json::Value::as_str).filter(|id| !id.is_empty())?;
        let kind = value.get("kind").and_then(serde_json::Value::as_str)?;
        (!RowKind::ALL.iter().any(|known| known.label() == kind)).then_some(kind)
    }

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
        if self.kind == RowKind::Control {
            ControlVerb::parse(&self.text)?;
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
