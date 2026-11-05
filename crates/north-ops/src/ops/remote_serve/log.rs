use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const REFUSED_EXIT: i32 = 126;
const COMMAND_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub ts: String,
    pub peer: String,
    pub command: String,
    pub verb: Option<String>,
    pub refusal: Option<String>,
    pub exit: i32,
    pub ms: u64,
}

pub struct Log {
    file: File,
}

pub fn path(home: &Path, peer: &str) -> PathBuf {
    home.join(".local/state/north/remote-serve").join(format!("{peer}.jsonl"))
}

pub fn clipped(command: &str) -> String {
    let end = (0..=command.len().min(COMMAND_BYTES)).rev().find(|end| command.is_char_boundary(*end)).unwrap_or(0);
    command[..end].to_string()
}

impl Log {
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).mode(0o600).open(path)?;
        Ok(Self { file })
    }

    pub fn append(&mut self, row: &Row) -> io::Result<()> {
        let mut line = serde_json::to_vec(row)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()
    }
}

pub struct Read {
    pub rows: Vec<Row>,
    pub unreadable: usize,
}

pub fn read(path: &Path) -> io::Result<Read> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let (mut rows, mut unreadable) = (Vec::new(), 0);
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str(line) {
            Ok(row) => rows.push(row),
            Err(_) => unreadable += 1,
        }
    }
    Ok(Read { rows, unreadable })
}
