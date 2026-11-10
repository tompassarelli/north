//! The tasks that carry frames into the provider and its output into the stream and log, each stopping at a frame or
//! line boundary when the host hands over.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::watch;

use super::{Activity, Fed, Frame, Lanes, STDIN_BLOCKED, STDIN_WRITE_TIMEOUT, Stage, Watcher};

pub(super) async fn feed_stdin(mut input: ChildStdin, mut lanes: Lanes, mut stage: watch::Receiver<Stage>) -> Fed {
    loop {
        let frame = tokio::select! {
            biased;
            Some(frame) = lanes.control.recv() => frame,
            Some(frame) = lanes.user.recv() => frame,
            Ok(_) = stage.wait_for(|stage| *stage == Stage::Handing) => break,
            else => break,
        };
        if write_frame(&mut input, frame).await.is_err() {
            break;
        }
    }
    Fed { input, lanes }
}

async fn write_frame(input: &mut ChildStdin, Frame { mut line, reply }: Frame) -> io::Result<()> {
    if !line.ends_with('\n') {
        line.push('\n');
    }
    let write = async {
        input.write_all(line.as_bytes()).await?;
        input.flush().await
    };
    tokio::pin!(write);
    match tokio::time::timeout(STDIN_WRITE_TIMEOUT, &mut write).await {
        Ok(written) => written,
        Err(_) => {
            if let Some(reply) = reply {
                let _ = reply.send(STDIN_BLOCKED.into());
            }
            write.await
        }
    }
}

/// Copies the provider's output into the stream a whole line at a time; once the host hands over, it stops at a line
/// boundary and returns the pipe, leaving whatever is unread in it for the next image.
pub(super) async fn copy_lines<R: AsyncRead + Unpin>(mut source: R, mut file: File, activity: Activity, watcher: Watcher, mut stage: watch::Receiver<Stage>) -> io::Result<Option<R>> {
    let mut pending = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = tokio::select! {
            biased;
            Ok(_) = stage.wait_for(|stage| *stage == Stage::Handing), if pending.is_empty() => return Ok(Some(source)),
            read = source.read(&mut chunk) => read?,
        };
        if watcher.output_keeps_awake() {
            activity.touch();
        }
        if read == 0 {
            if !pending.is_empty() {
                pending.push(b'\n');
                file.write_all(&pending)?;
                watcher.lines(&pending);
            }
            return Ok(None);
        }
        let start = pending.len();
        pending.extend_from_slice(&chunk[..read]);
        if let Some(position) = pending[start..].iter().rposition(|byte| *byte == b'\n') {
            let complete = start + position + 1;
            file.write_all(&pending[..complete])?;
            watcher.lines(&pending[..complete]);
            pending.drain(..complete);
        }
    }
}

pub(super) async fn copy_rotating<R: AsyncRead + Unpin>(mut source: R, path: PathBuf, limit: u64, mut stage: watch::Receiver<Stage>) -> io::Result<Option<R>> {
    let mut file = append(&path)?;
    let mut size = file.metadata()?.len();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = tokio::select! {
            biased;
            Ok(_) = stage.wait_for(|stage| *stage == Stage::Handing) => return Ok(Some(source)),
            read = source.read(&mut chunk) => read?,
        };
        if read == 0 {
            return Ok(None);
        }
        if size > 0 && size + read as u64 > limit {
            fs::rename(&path, path.with_extension("log.1"))?;
            file = append(&path)?;
            size = 0;
        }
        file.write_all(&chunk[..read])?;
        size += read as u64;
    }
}

pub(super) fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

