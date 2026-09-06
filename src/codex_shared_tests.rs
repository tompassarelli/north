use super::*;
use futures_util::{SinkExt, StreamExt};
use std::os::unix::fs::PermissionsExt;
use tokio::net::UnixListener;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn shared_socket_correlates_replies_and_releases_pending_requests() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("codex.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let request: Value =
            serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        socket.send(Message::Ping(vec![1].into())).await.unwrap();
        socket
            .send(Message::Text(
                json!({"id":request["id"],"method":"item/tool/requestUserInput","params":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        socket
            .send(Message::Text(
                json!({"id":request["id"],"result":{"accepted":true}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if matches!(message, Message::Text(_)) {
                break;
            }
        }
        socket.close(None).await.unwrap();
    });
    let socket = crate::codex_socket::connect(&format!("unix://{}", path.display()))
        .await
        .unwrap();
    let (rpc, mut events, driver) = Rpc::start_websocket(socket);
    assert_eq!(
        rpc.request("turn/start", json!({})).await.unwrap(),
        json!({"accepted":true})
    );
    assert_eq!(
        rpc::next_event(&mut events).await.unwrap()["method"],
        "item/tool/requestUserInput"
    );
    assert!(matches!(
        rpc.request("thread/read", json!({})).await,
        Err(NorthError::Protocol(_))
    ));
    server.await.unwrap();
    driver.await.unwrap();
}

#[tokio::test]
#[ignore = "requires NORTH_CODEX_TEST_BINARY pointing to the installed Codex binary"]
async fn installed_shared_socket_preserves_writer_across_client_reconnect() {
    let binary =
        std::env::var_os("NORTH_CODEX_TEST_BINARY").expect("an explicit installed Codex binary");
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("codex.sock");
    let endpoint = format!("unix://{}", path.display());
    let mut server = Command::new(binary)
        .args(["app-server", "--listen", &endpoint])
        .env("CODEX_HOME", directory.path())
        .env("CODEX_SQLITE_HOME", directory.path())
        .current_dir(directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(15), async {
        while !path.exists() {
            assert!(
                server.try_wait().unwrap().is_none(),
                "shared server exited before opening its socket"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let mut owner = Codex::connect_endpoint(&endpoint).await.unwrap();
    let thread = owner
        .start_new_conversation(directory.path())
        .await
        .unwrap();
    let mut attached = Codex::connect_endpoint(&endpoint).await.unwrap();
    assert_eq!(
        attached
            .resume_conversation(&thread)
            .await
            .unwrap()
            .snapshot
            .id,
        thread
    );
    attached.shutdown().await.unwrap();
    assert!(server.try_wait().unwrap().is_none());
    assert_eq!(
        owner
            .request("thread/read", json!({"threadId":thread}))
            .await
            .unwrap()["thread"]["id"],
        thread
    );
    let mut reconnected = Codex::connect_endpoint(&endpoint).await.unwrap();
    assert_eq!(
        reconnected
            .resume_conversation(&thread)
            .await
            .unwrap()
            .snapshot
            .id,
        thread
    );
    reconnected.shutdown().await.unwrap();
    owner.shutdown().await.unwrap();
    assert!(server.try_wait().unwrap().is_none());
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}
