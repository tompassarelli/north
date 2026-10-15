use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde_json::Value;

pub const WINDOW_SIZE: u64 = 1 << 20;

pub struct StreamTail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

pub fn read_window(file: &mut File, offset: u64, limit: u64) -> io::Result<Vec<u8>> {
    let file_len = file.metadata()?.len();
    let start = offset.min(file_len);
    let length = file_len.saturating_sub(start).min(limit) as usize;
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

impl StreamTail {
    pub fn open(path: PathBuf, offset: u64) -> Self {
        Self { path, offset, partial: Vec::new() }
    }

    pub fn frames(&mut self) -> io::Result<Vec<Value>> {
        self.read_new()?;
        let complete = self.partial.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        let frames = self.partial[..complete]
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect();
        self.partial.drain(..complete);
        self.offset += complete as u64;
        Ok(frames)
    }

    fn read_new(&mut self) -> io::Result<()> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let start = self.offset + self.partial.len() as u64;
        self.partial.extend(read_window(&mut file, start, WINDOW_SIZE)?);
        Ok(())
    }
}

pub fn stream_path(host_dir: &std::path::Path) -> PathBuf {
    host_dir.join("g1").join("stream.jsonl")
}

pub fn stream_len(host_dir: &std::path::Path) -> u64 {
    std::fs::metadata(stream_path(host_dir)).map_or(0, |metadata| metadata.len())
}
