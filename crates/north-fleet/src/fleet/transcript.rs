use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::SessionRecord;

pub const WINDOW_SIZE: u64 = 1 << 20;
const SESSION_STREAMS: &str = "session-streams";

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
        while !self.partial.contains(&b'\n') && self.read_new()? > 0 {}
        let complete = self.partial.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
        let frames = self.partial[..complete]
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect();
        self.partial.drain(..complete);
        self.offset += complete as u64;
        Ok(frames)
    }

    fn read_new(&mut self) -> io::Result<usize> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let start = self.offset + self.partial.len() as u64;
        let window = read_window(&mut file, start, WINDOW_SIZE)?;
        self.partial.extend(&window);
        Ok(window.len())
    }
}

pub fn stream_path(host_dir: &std::path::Path) -> PathBuf {
    host_dir.join("g1").join("stream.jsonl")
}

pub fn stream_len(host_dir: &std::path::Path) -> u64 {
    std::fs::metadata(stream_path(host_dir)).map_or(0, |metadata| metadata.len())
}

pub fn session_stream_path(host_dir: &Path, name: &str) -> PathBuf {
    host_dir.join("g1").join(SESSION_STREAMS).join(format!("{name}.jsonl"))
}

pub fn current_session_stream_index(host_dir: &Path, name: &str) -> Option<u64> {
    let directory = host_dir.join("g1").join(SESSION_STREAMS);
    if directory.join("indexing").exists() {
        return None;
    }
    let index_path = directory.join("index.json");
    let index = serde_json::from_slice::<StreamIndex>(&fs::read(index_path).ok()?).ok()?;
    (index.source == stream_path(host_dir) && index.indexed_to == stream_len(host_dir) && session_stream_path(host_dir, name).is_file()).then_some(index.indexed_to)
}

#[derive(Deserialize, Serialize)]
struct StreamIndex {
    source: PathBuf,
    indexed_to: u64,
}

pub fn ensure_session_streams(
    host_dir: &Path,
    records: &[SessionRecord],
    conversation: impl Fn(&Value) -> Option<String>,
) -> io::Result<u64> {
    let directory = host_dir.join("g1").join(SESSION_STREAMS);
    fs::create_dir_all(&directory)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(directory.join("index.lock"))?;
    flock(&lock, FlockOperation::LockExclusive).map_err(io::Error::other)?;
    let source = stream_path(host_dir);
    let source_len = std::fs::metadata(&source).map_or(0, |metadata| metadata.len());
    let sessions = stream_records(host_dir, records);
    if sessions.is_empty() {
        return Ok(source_len);
    }
    let interrupted = directory.join("indexing").exists();
    let (rebuild, start) = index_start(host_dir, &directory, source_len, &sessions, interrupted)?;
    fs::write(directory.join("indexing"), b"")?;
    let mut streams = open_session_streams(host_dir, &sessions, rebuild)?;
    let indexed_to = index_frames(&source, start, source_len, &sessions, &mut streams, &conversation)?;
    flush_session_streams(&mut streams)?;
    save_index(&directory, source, indexed_to)?;
    fs::remove_file(directory.join("indexing"))?;
    Ok(indexed_to)
}

fn stream_records<'a>(host_dir: &Path, records: &'a [SessionRecord]) -> Vec<&'a SessionRecord> {
    records.iter().filter(|record| record.host_dir == host_dir && record.conversation.is_some()).collect()
}

fn index_start(host_dir: &Path, directory: &Path, source_len: u64, sessions: &[&SessionRecord], interrupted: bool) -> io::Result<(bool, u64)> {
    let index_path = directory.join("index.json");
    let previous = fs::read(index_path).ok().and_then(|bytes| serde_json::from_slice::<StreamIndex>(&bytes).ok());
    let missing = sessions.iter().any(|record| !session_stream_path(host_dir, &record.name).is_file());
    let rebuild = interrupted || previous.as_ref().is_none_or(|index| index.source != stream_path(host_dir))
        || previous.as_ref().is_some_and(|index| index.indexed_to > source_len)
        || missing;
    let start = if rebuild { sessions.iter().map(|record| record.stream_offset).min().unwrap_or(source_len).min(source_len) } else { previous.as_ref().map_or(0, |index| index.indexed_to) };
    Ok((rebuild, start))
}

fn open_session_streams<'a>(host_dir: &Path, sessions: &[&'a SessionRecord], rebuild: bool) -> io::Result<HashMap<&'a str, BufWriter<File>>> {
    sessions.iter().map(|record| {
        let file = OpenOptions::new().create(true).write(true).append(!rebuild).truncate(rebuild).open(session_stream_path(host_dir, &record.name))?;
        Ok((record.name.as_str(), BufWriter::new(file)))
    }).collect()
}

fn index_frames(
    source: &Path,
    start: u64,
    source_len: u64,
    sessions: &[&SessionRecord],
    streams: &mut HashMap<&str, BufWriter<File>>,
    conversation: &impl Fn(&Value) -> Option<String>,
) -> io::Result<u64> {
    if start >= source_len {
        return Ok(start);
    }
    let mut file = File::open(source)?;
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut indexed_to = start;
    while read_complete_line(&mut reader, &mut line)? {
        if let Ok(frame) = serde_json::from_slice::<Value>(&line) {
            index_frame(&frame, &line, indexed_to, sessions, streams, conversation)?;
        }
        indexed_to += line.len() as u64;
    }
    Ok(indexed_to)
}

fn read_complete_line(reader: &mut BufReader<File>, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    let read = reader.read_until(b'\n', line)?;
    Ok(read > 0 && line.last() == Some(&b'\n'))
}

fn index_frame(
    frame: &Value,
    line: &[u8],
    offset: u64,
    sessions: &[&SessionRecord],
    streams: &mut HashMap<&str, BufWriter<File>>,
    conversation: &impl Fn(&Value) -> Option<String>,
) -> io::Result<()> {
    let Some(conversation) = conversation(frame) else { return Ok(()) };
    for record in sessions.iter().filter(|record| record.conversation.as_deref() == Some(conversation.as_str()) && offset >= record.stream_offset) {
        if let Some(stream) = streams.get_mut(record.name.as_str()) {
            stream.write_all(line)?;
        }
    }
    Ok(())
}

fn flush_session_streams(streams: &mut HashMap<&str, BufWriter<File>>) -> io::Result<()> {
    for stream in streams.values_mut() {
        stream.flush()?;
    }
    Ok(())
}

fn save_index(directory: &Path, source: PathBuf, indexed_to: u64) -> io::Result<()> {
    let temporary = directory.join("index.json.tmp");
    fs::write(&temporary, serde_json::to_vec(&StreamIndex { source, indexed_to }).map_err(io::Error::other)?)?;
    fs::rename(temporary, directory.join("index.json"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(root: &Path, name: &str, conversation: &str, stream_offset: u64) -> SessionRecord {
        serde_json::from_value(json!({
            "name": name,
            "provider": "codex",
            "account": "pooled",
            "cwd": "/tmp",
            "model": null,
            "created_ms": 0,
            "host_pid": 1,
            "conversation": conversation,
            "host_dir": root,
            "stream_offset": stream_offset
        })).unwrap()
    }

    #[test]
    fn per_session_stream_index_keeps_shared_host_frames_separate_and_incremental() {
        let root = tempfile::tempdir().unwrap();
        let first = b"{\"thread\":\"a\",\"text\":\"first\"}\n";
        let second = b"{\"thread\":\"b\",\"text\":\"other\"}\n";
        let third = b"{\"thread\":\"a\",\"text\":\"third\"}\n";
        let stream = stream_path(root.path());
        fs::create_dir_all(stream.parent().unwrap()).unwrap();
        fs::write(&stream, [first.as_slice(), second.as_slice(), third.as_slice()].concat()).unwrap();
        let records = [record(root.path(), "a", "a", 0), record(root.path(), "b", "b", first.len() as u64)];
        let conversation = |frame: &Value| frame["thread"].as_str().map(str::to_owned);
        let indexed = ensure_session_streams(root.path(), &records, conversation).unwrap();
        assert_eq!(indexed, stream_len(root.path()));
        assert_eq!(fs::read_to_string(session_stream_path(root.path(), "a")).unwrap(), format!("{}{}", std::str::from_utf8(first).unwrap(), std::str::from_utf8(third).unwrap()));
        assert_eq!(fs::read_to_string(session_stream_path(root.path(), "b")).unwrap(), std::str::from_utf8(second).unwrap());
        let later = b"{\"thread\":\"a\",\"text\":\"later\"}\n";
        OpenOptions::new().append(true).open(stream).unwrap().write_all(later).unwrap();
        let indexed = ensure_session_streams(root.path(), &records, conversation).unwrap();
        assert_eq!(indexed, stream_len(root.path()));
        assert!(fs::read_to_string(session_stream_path(root.path(), "a")).unwrap().ends_with(std::str::from_utf8(later).unwrap()));
        assert_eq!(fs::read_to_string(session_stream_path(root.path(), "b")).unwrap(), std::str::from_utf8(second).unwrap());
    }
}
