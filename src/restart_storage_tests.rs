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
