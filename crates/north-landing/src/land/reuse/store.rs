use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub lane_head: String,
    pub main: String,
    pub input: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    key: Key,
    verdict: Verdict,
}

impl Key {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(':');
        let key = Self { lane_head: parts.next()?.to_owned(), main: parts.next()?.to_owned(), input: parts.next()?.to_owned() };
        (parts.next().is_none() && [&key.lane_head, &key.main, &key.input].iter().all(|part| !part.is_empty())).then_some(key)
    }

    pub fn file_name(&self) -> String {
        let mut hasher = Sha256::new();
        for part in [&self.lane_head, &self.main, &self.input] {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{digest}.json")
    }
}

pub fn record(state: &Path, key: &Key, verdict: Verdict) -> io::Result<()> {
    fs::create_dir_all(state)?;
    let entry = Entry { key: key.clone(), verdict };
    let mut file = tempfile::NamedTempFile::new_in(state)?;
    file.write_all(&serde_json::to_vec(&entry).map_err(io::Error::other)?)?;
    file.as_file().sync_all()?;
    file.persist(state.join(key.file_name())).map_err(|error| error.error)?;
    Ok(())
}

pub fn lookup(state: &Path, key: &Key) -> Option<Verdict> {
    let entry: Entry = serde_json::from_slice(&fs::read(state.join(key.file_name())).ok()?).ok()?;
    (entry.key == *key).then_some(entry.verdict)
}
