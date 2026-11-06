//! The pull hand-over of a queued agent heavy run: a host that holds no key to its peers (the gex45 access plan) is polled instead, and a peer with free slots takes the oldest queued run over the connection it opened. The peer runs nothing the lane's host did not offer, and checks every field of the offer before a word of it reaches a shell.

use std::io::{self, Read, Write};

use serde_json::{Value, json};

use super::build_offload::{MAX_DIFF_BYTES, TOOLCHAIN_PROGRAMS};

pub const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TIMEOUT_SECONDS: u64 = 4 * 3600;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_WORD_BYTES: usize = 4096;
const MAX_COMMAND_WORDS: usize = 256;

/// A queued run as its host offers it: the lane to rebuild and the toolchain command to run in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub repo: String,
    pub head: String,
    pub name: String,
    pub subdirectory: String,
    pub owner: String,
    pub timeout_seconds: u64,
    pub command: Vec<String>,
    pub bundle_bytes: u64,
    pub patch_bytes: u64,
    pub returned: String,
}

/// Whether a queued run may be handed over at all: a toolchain program, so a peer never executes a shell string the queue's host composed.
pub fn pullable(command: &[String]) -> bool {
    command.first().is_some_and(|program| TOOLCHAIN_PROGRAMS.contains(&program.rsplit('/').next().unwrap_or_default()))
}

fn path_word(word: &str, empty: bool) -> bool {
    let plain = word.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'));
    let climbs = word.split('/').any(|part| part == "..") || word.starts_with('/') || word.starts_with('-');
    (empty || !word.is_empty()) && word.len() <= MAX_WORD_BYTES && plain && !climbs
}

impl Offer {
    /// Why a peer must not run this offer, None when every field is one a lane's host sends.
    pub fn refusal(&self) -> Option<String> {
        let hex = |head: &str| matches!(head.len(), 40 | 64) && head.bytes().all(|byte| byte.is_ascii_hexdigit());
        let checks: [(&str, bool); 10] = [
            ("repo", path_word(&self.repo, false)),
            ("head", hex(&self.head)),
            ("name", path_word(&self.name, false) && !self.name.contains('/')),
            ("subdirectory", path_word(&self.subdirectory, true)),
            ("owner", !self.owner.is_empty() && self.owner.len() <= 256 && !self.owner.chars().any(char::is_control)),
            ("timeout", (1..=MAX_TIMEOUT_SECONDS).contains(&self.timeout_seconds)),
            ("command", pullable(&self.command) && self.command.len() <= MAX_COMMAND_WORDS && self.command.iter().all(|word| word.len() <= MAX_WORD_BYTES && !word.contains('\0'))),
            ("bundle", self.bundle_bytes <= MAX_BUNDLE_BYTES),
            ("patch", self.patch_bytes <= MAX_DIFF_BYTES as u64),
            ("returned", self.returned.is_empty() || self.returned.strip_prefix("NORTH-OFFLOAD-RETURNED-").is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-'))),
        ];
        checks.iter().find(|(_, fine)| !fine).map(|(field, _)| format!("the offer's {field} is not one a lane's host sends"))
    }

    pub fn to_line(&self) -> String {
        let value = json!({
            "repo": self.repo, "head": self.head, "name": self.name, "subdirectory": self.subdirectory, "owner": self.owner,
            "timeoutSeconds": self.timeout_seconds, "command": self.command, "bundleBytes": self.bundle_bytes, "patchBytes": self.patch_bytes, "returned": self.returned,
        });
        format!("{value}\n")
    }

    pub fn from_line(line: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        let text = |key: &str| value.get(key)?.as_str().map(str::to_owned);
        let number = |key: &str| value.get(key)?.as_u64();
        let command = value.get("command")?.as_array()?.iter().map(|word| word.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>()?;
        Some(Self {
            repo: text("repo")?,
            head: text("head")?,
            name: text("name")?,
            subdirectory: text("subdirectory")?,
            owner: text("owner")?,
            timeout_seconds: number("timeoutSeconds")?,
            command,
            bundle_bytes: number("bundleBytes")?,
            patch_bytes: number("patchBytes")?,
            returned: text("returned").unwrap_or_default(),
        })
    }
}

/// What the taking peer sends back over the connection it opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Started(String),
    NotSynced(String),
    Out(Vec<u8>),
    Err(Vec<u8>),
    Exit(u8),
}

impl Frame {
    pub fn write(&self, sink: &mut impl Write) -> io::Result<()> {
        let (tag, payload): (u8, &[u8]) = match self {
            Self::Started(host) => (b's', host.as_bytes()),
            Self::NotSynced(why) => (b'n', why.as_bytes()),
            Self::Out(bytes) => (b'o', bytes),
            Self::Err(bytes) => (b'e', bytes),
            Self::Exit(code) => (b'x', std::slice::from_ref(code)),
        };
        let length = u32::try_from(payload.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        sink.write_all(&[tag])?;
        sink.write_all(&length.to_be_bytes())?;
        sink.write_all(payload)?;
        sink.flush()
    }

    /// The next frame, None at a clean end of the stream between frames.
    pub fn read(source: &mut impl Read) -> io::Result<Option<Self>> {
        let mut tag = [0u8; 1];
        if source.read(&mut tag)? == 0 {
            return Ok(None);
        }
        let mut length = [0u8; 4];
        source.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_FRAME_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame over the size limit"));
        }
        let mut payload = vec![0u8; length];
        source.read_exact(&mut payload)?;
        let text = || String::from_utf8_lossy(&payload).into_owned();
        Ok(Some(match tag[0] {
            b's' => Self::Started(text()),
            b'n' => Self::NotSynced(text()),
            b'o' => Self::Out(payload),
            b'e' => Self::Err(payload),
            b'x' if payload.len() == 1 => Self::Exit(payload[0]),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown frame")),
        }))
    }
}
