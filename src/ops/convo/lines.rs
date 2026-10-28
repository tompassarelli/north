use std::fs::File;
use std::io::{self, Read};
use std::process::{Child, ChildStdout, Command, Stdio};

use sha2::{Digest, Sha256};

pub(super) const LINE_CAP: usize = 1 << 20;
pub(super) const CHUNK: usize = 1 << 20;
pub(super) const HEAD_BYTES: usize = 4096;

/// Calls `visit(start, end, line)` once per complete line; `line` is `None` past `cap` bytes.
pub(super) fn scan_lines<R: Read>(
    reader: &mut R,
    start: u64,
    chunk: usize,
    cap: usize,
    mut visit: impl FnMut(u64, u64, Option<&[u8]>) -> io::Result<()>,
) -> io::Result<()> {
    let mut buffer = vec![0u8; chunk];
    let mut line = Vec::new();
    let mut length = 0usize;
    let mut line_start = start;
    let mut base = start;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        let data = &buffer[..count];
        let mut position = 0;
        while position < count {
            let newline = data[position..].iter().position(|byte| *byte == b'\n');
            let end = newline.map_or(count, |offset| position + offset);
            length += end - position;
            if length <= cap {
                line.extend_from_slice(&data[position..end]);
            }
            let Some(offset) = newline else { break };
            let finish = base + (position + offset) as u64 + 1;
            visit(line_start, finish, (length <= cap).then_some(line.as_slice()))?;
            line.clear();
            length = 0;
            line_start = finish;
            position += offset + 1;
        }
        base += count as u64;
    }
}

pub(super) struct Source {
    reader: Box<dyn Read>,
    child: Option<Child>,
    head: Option<Vec<u8>>,
}

impl Source {
    pub(super) fn open(zstd: &str, disk: &str, start: u64) -> io::Result<Self> {
        if disk.ends_with(".zst") {
            let mut child = Command::new(zstd).args(["-dcq", "--", disk]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
            let stdout: ChildStdout = child.stdout.take().ok_or_else(|| io::Error::other("zstd gave no stdout"))?;
            return Ok(Self { reader: Box::new(stdout), child: Some(child), head: Some(Vec::new()) });
        }
        let mut file = File::open(disk)?;
        if start > 0 {
            io::Seek::seek(&mut file, io::SeekFrom::Start(start))?;
        }
        Ok(Self { reader: Box::new(file), child: None, head: None })
    }

    pub(super) fn head(&self) -> String {
        hex(&Sha256::digest(self.head.as_deref().unwrap_or(&[])))
    }

    pub(super) fn close(self) -> bool {
        drop(self.reader);
        self.child.is_none_or(|mut child| child.wait().is_ok_and(|status| status.success()))
    }
}

impl Read for Source {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(out)?;
        if let Some(head) = &mut self.head {
            let room = HEAD_BYTES.saturating_sub(head.len());
            head.extend_from_slice(&out[..count.min(room)]);
        }
        Ok(count)
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hash of the first `count` bytes: an append keeps it, a rewrite changes it.
pub(super) fn head_hash(path: &str, count: usize) -> Option<String> {
    let mut bytes = vec![0u8; count];
    File::open(path).ok()?.read_exact(&mut bytes).ok()?;
    Some(hex(&Sha256::digest(&bytes)))
}
