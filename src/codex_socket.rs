//! The explicitly selected local Codex transport. Its server is owned separately.

use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::time::Duration;

use tokio::net::UnixStream;
use tokio::time::timeout;
use tokio_tungstenite::WebSocketStream;

use crate::error::{NorthError, NorthResult};

pub async fn connect(endpoint: &str) -> NorthResult<WebSocketStream<UnixStream>> {
    let path = endpoint
        .strip_prefix("unix://")
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            NorthError::Protocol(
                "NORTH_CODEX_ENDPOINT requires unix:///absolute/socket/path".into(),
            )
        })?;
    let parent = path.parent().ok_or_else(|| {
        NorthError::Protocol("NORTH_CODEX_ENDPOINT has no socket directory".into())
    })?;
    let user = rustix::process::geteuid().as_raw();
    let directory = std::fs::metadata(parent)?;
    let socket = std::fs::symlink_metadata(path)?;
    if !directory.is_dir()
        || directory.uid() != user
        || directory.mode() & 0o077 != 0
        || !socket.file_type().is_socket()
        || socket.uid() != user
    {
        return Err(NorthError::Protocol(
            "Codex socket must belong to the current user inside a private directory".into(),
        ));
    }
    timeout(Duration::from_secs(10), async {
        let stream = UnixStream::connect(path).await?;
        if stream.peer_cred()?.uid() != user {
            return Err(NorthError::Protocol(
                "Codex socket peer belongs to another user".into(),
            ));
        }
        // The HTTP authority is only the WebSocket handshake host: all bytes use
        // the already connected Unix socket, with no TCP or DNS operation.
        let (socket, _) = tokio_tungstenite::client_async("ws://localhost/", stream)
            .await
            .map_err(|error| {
                NorthError::Protocol(format!("Codex socket handshake failed: {error}"))
            })?;
        Ok(socket)
    })
    .await
    .map_err(|_| NorthError::Protocol("Codex socket connection timed out".into()))?
}
