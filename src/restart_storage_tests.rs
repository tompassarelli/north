use super::*;
use std::fs;
use std::io::Write;

fn attach(app: &mut App, bytes: &[u8]) -> AttachmentIdentity {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    app.state.retain_image_file(file.path()).unwrap();
    let identity = app.state.attach_image().unwrap();
    app.composer.attach_image(identity, file);
    identity
}

#[tokio::test]
async fn restart_storage_fresh_process() {
    if let Some(root) = std::env::var_os("NORTH_RESTART_FIXTURE") {
        let root = PathBuf::from(root);
        let cwd = root.join("workspace");
        let mut app = App::open(cwd.clone()).unwrap();
        if std::env::var("NORTH_RESTART_PHASE").unwrap() == "write" {
            app.state.request_new_conversation().unwrap();
            app.state.settle_new_conversation("restart-thread").unwrap();
            app.state.create_goal("Saved goal", "Keep its original identity").unwrap();
            fs::write(root.join("goal-proof"), format!("{:?}", app.state.active_goal().unwrap())).unwrap();
            attach(&mut app, b"queued image bytes");
            let submission = app.composer.take_submission();
            let queued = app.state.queue_input("queued text").unwrap();
            app.pending_images.insert(queued, submission.into_images());
            assert_eq!(queued, 1);
            let uncertain = app.state.retain_direct("unconfirmed text").unwrap();
            app.state.bind_input_client_id(uncertain, "stable-original-client").unwrap();
            assert_eq!(uncertain, 2);
            app.state.submit_queued(uncertain).unwrap();
            app.composer.insert_text("saved draft");
            let draft_image = attach(&mut app, b"draft image bytes");
            assert_eq!(draft_image.number(), 2);
            app.handle_composer_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE)).await;
            assert!(App::open(cwd).err().unwrap().to_string().contains("another North"));
            // A real process exit releases custody without running App destructors.
            std::process::exit(0);
        }
        assert_eq!(format!("{:?}", app.state.active_goal().unwrap()), fs::read_to_string(root.join("goal-proof")).unwrap());
        assert_eq!(app.composer.text(), "saved draft [Image #2] !");
        assert_eq!(app.state.conversation("restart-thread").unwrap().draft_attachments, vec![AttachmentIdentity(2)]);
        let draft = app.composer.take_submission();
        assert_eq!(fs::read(&draft.image_paths(&[AttachmentIdentity(2)]).unwrap()[0]).unwrap(), b"draft image bytes");
        assert_eq!(app.state.pending_inputs().len(), 2);
        assert_eq!(app.state.pending_inputs()[0].number, 1);
        assert_eq!(app.state.pending_inputs()[0].text, "queued text");
        assert_eq!(app.state.pending_inputs()[0].status, "paused");
        assert_eq!(app.state.pending_inputs()[0].attachments, vec![AttachmentIdentity(1)]);
        assert_eq!(fs::read(&app.pending_image_paths(1).unwrap()[0]).unwrap(), b"queued image bytes");
        assert_eq!(app.state.pending_inputs()[1].status, "delivery unknown");
        app.state.begin_conversation_reconciliation("restart-thread").unwrap();
        app.state.finish_reconnect(true).unwrap();
        app.state.observe_input_acceptance("restart-thread", "unconfirmed text").unwrap();
        assert_eq!(app.state.pending_inputs()[1].status, "delivery unknown");
        app.dispatch_ready_work().unwrap();
        assert!(app.turns.is_empty() && app.steering.is_empty());
        app.state.observe_input_acceptance("restart-thread", "stable-original-client").unwrap();
        assert_eq!(app.state.pending_inputs()[1].status, "accepted");
        assert_eq!(attach(&mut app, b"next image").number(), 3);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("workspace")).unwrap();
    for phase in ["write", "read"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restart_storage_tests::restart_storage_fresh_process", "--nocapture"])
            .env("NORTH_RESTART_FIXTURE", root.path()).env("NORTH_RESTART_PHASE", phase)
            .env("XDG_STATE_HOME", root.path().join("state"))
            .output().unwrap();
        assert!(output.status.success(), "{phase}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn history_and_full_message_reopen_without_clipping() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("workspace");
    fs::create_dir(&cwd).unwrap();
    let storage = root.path().join("state");
    let text = "完整消息🙂".repeat(28_000);
    assert!(text.len() > 446_848);
    let mut app = App::open_stored(cwd.clone(), &storage).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("long-message-thread").unwrap();
    app.state.open_history(&[codex::ConversationOption {
        id: "long-message-thread".into(),
        title: text.clone(),
        preview: text.clone(),
        current: true,
    }], "", false).unwrap();
    assert_eq!(app.state.menu().rows[0].label, text);
    app.state.close_menu().unwrap();
    app.state.observe_chat_item(&clause_state::ChatEntryInput {
        conversation: "long-message-thread", turn: "long-turn", key: "long-item",
        kind: "userMessage", text: &text, status: "completed", append: false,
    }).unwrap();
    drop(app);
    let reopened = App::open_stored(cwd, &storage).unwrap();
    assert_eq!(reopened.state.active_conversation(), Some("long-message-thread"));
    let entry = reopened.state.chat().iter().find(|entry| entry.key == "long-item").unwrap();
    assert_eq!(entry.text, text);
}

#[test]
fn restart_storage_rejects_corruption_and_source_mismatch_without_replacement() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("workspace");
    fs::create_dir(&cwd).unwrap();
    let storage = root.path().join("state");
    drop(App::open_stored(cwd.clone(), &storage).unwrap());
    let directory = fs::read_dir(&storage).unwrap().next().unwrap().unwrap().path();
    let world = directory.join("world");
    fs::write(&world, b"truncated").unwrap();
    assert!(App::open_stored(cwd.clone(), &storage).is_err());
    assert_eq!(fs::read(&world).unwrap(), b"truncated");
    let mut source = include_bytes!("../clause/north.clause").to_vec();
    source.push(b'\n');
    let changed = clause_workbench::ResidentSourceWorkbenchV1::open_continuous(&source).unwrap()
        .checkpoint_admitted().unwrap();
    fs::write(&world, &changed).unwrap();
    assert!(App::open_stored(cwd, &storage).is_err());
    assert_eq!(fs::read(&world).unwrap(), changed);
}

#[tokio::test]
async fn restart_storage_explicit_resume_propagates_refusal_and_preserves_identity() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use tokio_tungstenite::tungstenite::Message;

    let Some(root) = std::env::var_os("NORTH_RESUME_FIXTURE") else {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.path().join("workspace")).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restart_storage_tests::restart_storage_explicit_resume_propagates_refusal_and_preserves_identity", "--nocapture"])
            .env("NORTH_RESUME_FIXTURE", root.path())
            .env("XDG_STATE_HOME", root.path().join("state"))
            .env("NORTH_CODEX_ENDPOINT", format!("unix://{}/codex.sock", root.path().display()))
            .output().unwrap();
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    };
    let root = PathBuf::from(root);
    let cwd = root.join("workspace");
    let mut app = App::open(cwd.clone()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("saved-thread").unwrap();
    app.state.observe_settings("saved-thread", "fixture-model", "high").unwrap();
    drop(app);
    let listener = tokio::net::UnixListener::bind(root.join("codex.sock")).unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(message) = socket.next().await {
                let message = match message {
                    Err(tokio_tungstenite::tungstenite::Error::Protocol(
                        tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake
                    )) => break,
                    other => other.unwrap(),
                };
                if message.is_close() { break; }
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if request.get("id").is_none() { continue; }
                let result = match request["method"].as_str().unwrap() {
                    "initialize" => json!({}),
                    "model/list" => json!({"data":[{"model":"fixture-model", "description":"Fixture", "defaultReasoningEffort":"high", "supportedReasoningEfforts":[{"reasoningEffort":"high", "description":"Fixture"}], "isDefault":true}]}),
                    "thread/resume" => {
                        let requested = request["params"]["threadId"].as_str().unwrap();
                        if requested == "missing-thread" {
                            socket.send(Message::Text(json!({"id":request["id"],"error":{"code":-32602,"message":"No such conversation"}}).to_string().into())).await.unwrap();
                            continue;
                        }
                        let id = if requested == "wrong-response" { "unrequested-thread" } else { requested };
                        json!({"thread":{"id":id,"turns":[]},"model":"fixture-model","reasoningEffort":"high"})
                    }
                    method => panic!("Unexpected request {method}; explicit resume must not choose a different conversation"),
                };
                socket.send(Message::Text(json!({"id":request["id"],"result":result}).to_string().into())).await.unwrap();
            }
        }
    });
    tokio::time::timeout(Duration::from_secs(20), async {
        for requested in ["missing-thread", "wrong-response", "requested-thread"] {
            let mut app = App::open(cwd.clone()).unwrap();
            assert_eq!(app.state.connection_state(), "disconnected");
            let result = app.ensure_codex(Some(requested)).await;
            if requested == "requested-thread" {
                result.unwrap();
                assert_eq!(app.state.active_conversation(), Some(requested));
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains(if requested == "missing-thread" { "No such conversation" } else { "different conversation" }), "{error}: {}", app.displayed_messages());
                assert_eq!(app.state.active_conversation(), Some("saved-thread"));
            }
            app.shutdown().await;
        }
        server.await.unwrap();
    }).await.unwrap();
}
