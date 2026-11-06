use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Machine,
    HumanSession,
    Retired,
    Undeclared,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Machine => "machine",
            Kind::HumanSession => "human-session",
            Kind::Retired => "retired",
            Kind::Undeclared => "undeclared",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Renewal {
    Never,
    Command(Vec<String>),
    Tom(String),
}

impl Renewal {
    pub fn describe(&self) -> String {
        match self {
            Renewal::Never => "none needed".into(),
            Renewal::Command(words) => format!("run {}", words.join(" ")),
            Renewal::Tom(cause) => format!("Tom: {cause}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credential {
    pub name: String,
    pub source: String,
    pub usage: String,
    pub kind: Kind,
    pub fingerprint: Option<String>,
    pub expires: Option<u64>,
    pub renewal: Renewal,
    pub note: Option<String>,
}

pub fn fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(6).map(|byte| format!("{byte:02x}")).collect()
}

pub fn day(seconds: u64) -> String {
    i64::try_from(seconds).ok().and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok()).map_or_else(|| "?".into(), |moment| moment.date().to_string())
}


pub fn modified(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|elapsed| elapsed.as_secs())
}
