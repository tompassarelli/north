use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde_json::Value;

pub struct StreamTail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
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
        file.seek(SeekFrom::Start(self.offset + self.partial.len() as u64))?;
        file.read_to_end(&mut self.partial)?;
        Ok(())
    }
}

pub fn stream_path(host_dir: &std::path::Path) -> PathBuf {
    host_dir.join("g1").join("stream.jsonl")
}

pub fn stream_len(host_dir: &std::path::Path) -> u64 {
    std::fs::metadata(stream_path(host_dir)).map_or(0, |metadata| metadata.len())
}
