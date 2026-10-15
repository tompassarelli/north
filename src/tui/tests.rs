use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::command_surface::Picker;
use crate::composer::{Composer, Submission};
use crate::error::NorthResult;
use crate::north_state::{NorthPhase, NorthState};
use crate::{agent_catalog, codex, north_state, prompts, references, rpc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::style::{Color, Modifier};

use super::app::*;
use super::render::*;
use super::terminal::*;

#[tokio::test]
async fn reconnect_receipts_retain_direct_images_and_require_exact_saved_identity() {
    use tokio::io::{AsyncBufReadExt, BufReader, duplex, split};
    use serde_json::Value;
    let mut app = App::ephemeral(PathBuf::from("/tmp/north-receipt-test")).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("receipt-thread").unwrap();
    let image = app.state.attach_image().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    app.composer.insert_text("same text");
    app.composer.attach_image(image, file);
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection);
    app.events = Some(events);
    let peer = tokio::spawn(async move {
        let mut lines = BufReader::new(server).lines();
        let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "turn/start");
        request["params"]["clientUserMessageId"].as_str().unwrap().to_owned()
    });
    let input = app.composer.take_submission();
    app.accept_submission(input).await;
    app.composer.insert_text("next draft");
    let client_id = peer.await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !app.turns.is_empty() { app.collect_finished_turn().await; tokio::task::yield_now().await; }
    }).await.unwrap();
    driver.await.unwrap();
    let number = app.state.pending_inputs()[0].number;
    assert_eq!(app.state.pending_inputs()[0].status, "delivery unknown");
    assert_eq!(app.pending_image_paths(number).unwrap(), vec![path.clone()]);
    assert!(path.exists());
    assert_eq!(app.composer.text(), "next draft");
    app.state.observe_input_acceptance("other-thread", &client_id).unwrap();
    app.state.observe_input_acceptance("receipt-thread", "same text").unwrap();
    assert_eq!(app.state.pending_inputs()[0].status, "delivery unknown");
    app.state.observe_input_acceptance("receipt-thread", &client_id).unwrap();
    assert_eq!(app.state.pending_inputs()[0].status, "accepted");
    app.forget_input(number);
    assert!(!path.exists());
    assert!(app.state.pending_inputs().is_empty());
}

#[test]
fn usage_display_distinguishes_missing_values_and_preserves_conversation_scope() {
    use serde_json::json;
    let mut app = accepted_frame_app();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("usage-thread").unwrap();
    let conversation = app.state.active_conversation().unwrap().to_owned();
    let context = serde_json::from_value(json!({"threadId":conversation,"tokenUsage":{
        "last":{"totalTokens":500},"total":{"totalTokens":2300},"modelContextWindow":1000,
    }})).unwrap();
    app.state.observe_context_usage(&context).unwrap();
    let other = serde_json::from_value(json!({"threadId":"other","tokenUsage":{
        "last":{"totalTokens":990},"total":{"totalTokens":9999},"modelContextWindow":1000,
    }})).unwrap();
    app.state.observe_context_usage(&other).unwrap();
    let quota = serde_json::from_value(json!({"rateLimits":{
        "limitId":"codex","limitName":"Codex","primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":2200},
        "secondary":{"usedPercent":0,"windowDurationMins":null,"resetsAt":null},
    },"rateLimitResetCredits":{"availableCount":2}})).unwrap();
    app.state.observe_account_usage(&quota, 1000).unwrap();
    app.state.accept_input("/usage").unwrap();
    app.state.clear_host_effect().unwrap();
    let screen = render_text(&mut app, 110, 28);
    let values = app.state.usage().rows.iter().map(|row| (row.value as u64, row.known)).collect::<Vec<_>>();
    assert_eq!(values, [(50, true), (2300, true), (75, true), (300, true), (20, true), (100, true), (0, false), (0, false), (2, true)]);
    for text in ["50%", "2300", "75%", "20", "2"] {
        assert!(screen.contains(text), "Missing {text:?}: {screen}");
    }
    assert!(!screen.contains("9999"));
    let count = app.state.usage().rows.len();
    app.state.observe_account_usage(&quota, 1060).unwrap();
    assert_eq!(app.state.usage().rows.len(), count);
    assert_eq!(app.state.usage().rows[4].value, 19.0);
    assert!(render_text(&mut app, 110, 28).contains("19"));
}

#[tokio::test]
async fn usage_compaction_waits_for_completion_without_requiring_an_answer() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let mut app = App::ephemeral(tempfile::tempdir().unwrap().path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread").unwrap();
    app.state.observe_settings("thread", "fixture-model", "high").unwrap();
    app.composer.insert_text("keep my draft");
    let queued = app.state.queue_input("afterwards").unwrap();
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let peer = tokio::spawn(async move {
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "thread/compact/start");
        assert_eq!(request["params"]["threadId"], "thread");
        for event in [
            json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"compact"}}}),
            json!({"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"compact","status":"completed","items":[]}}}),
            json!({"id":request["id"],"result":{}}),
        ] { writer.write_all(format!("{event}\n").as_bytes()).await.unwrap(); }
        assert!(lines.next_line().await.unwrap().is_none());
    });
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut command = Composer::new();
        command.insert_text("/compact");
        app.accept_submission(command.take_submission()).await;
        assert!(app.is_working());
        assert_eq!(app.state.prepare_queued_input().unwrap(), None);
        while !app.turns.is_empty() { app.collect_finished_turn().await; tokio::task::yield_now().await; }
        assert_eq!(app.state.phase(), NorthPhase::Completed);
        assert!(app.turns.is_empty());
        assert!(!app.is_working());
        assert_eq!(app.composer.text(), "keep my draft");
        assert_eq!(app.state.prepare_queued_input().unwrap(), Some(queued));
    }).await.unwrap();
    connection.close().await;
    peer.await.unwrap();
    driver.await.unwrap();
}

#[tokio::test]
async fn saved_workspace_handles_navigation_while_reconnection_is_pending() {
    let mut app = App::ephemeral(PathBuf::from("/tmp/north-navigation-test")).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("saved-thread").unwrap();
    app.state.observe_settings("saved-thread", "gpt-example", "high").unwrap();
    app.state.connection_lost().unwrap();
    assert_eq!(app.state.attached_conversations().count(), 1);
    app.reconnect_task = Some(tokio::spawn(std::future::pending::<NorthResult<Reconnection>>()));
    app.ensure_codex(None).await.unwrap();
    assert!(navigate_view(&mut app.state, &KeyCode::Tab, true).unwrap());
    assert_eq!(app.state.active_view(), "goals");
    assert!(app.reconnect_task.as_ref().is_some_and(|task| !task.is_finished()));
    app.shutdown().await;
}

#[test]
fn idle_polls_preserve_the_revision_and_new_input_reactivates_dispatch() {
    let mut app = accepted_frame_app();
    let initial = app.state.revision();
    app.dispatch_ready_work().unwrap();
    assert_eq!(app.state.revision(), initial);
    app.state.navigate_view(true).unwrap();
    let revision = app.state.revision();
    app.dispatch_ready_work().unwrap();
    assert_eq!(app.state.revision(), revision);
    app.dispatch_ready_work().unwrap();
    assert_eq!(app.state.revision(), revision);
    app.state.create_goal("Continue", "Retain this update").unwrap();
    assert_ne!(app.state.revision(), revision);
    app.dispatch_ready_work().unwrap();
    assert_eq!(app.dispatched_revision, app.state.revision());
}

#[test]
fn transcript_controls_scroll_filter_and_changes_without_changing_the_draft() {
    let mut app = App::ephemeral(PathBuf::from("/tmp/north-transcript-test")).unwrap();
    app.composer.insert_text("keep my draft");
    for number in 0..20 { app.record_chat(Speaker::Operator, format!("message {number}")); }
    let screen = render_text(&mut app, 110, 12);
    assert!(screen.contains("message 19"), "{screen}");
    app.handle_transcript_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL)).unwrap();
    let screen = render_text(&mut app, 110, 12);
    assert!(screen.contains("message 0"), "{screen}");
    assert!(!screen.contains("message 19"), "{screen}");
    app.handle_transcript_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL)).unwrap();
    app.record_chat(Speaker::FileChange, "src/example.rs\n-old\n+ÅNGSTRÖM".into());
    app.record_chat(Speaker::North, "Ångström discussed in prose".into());
    app.handle_transcript_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)).unwrap();
    for character in "ångström".chars() {
        app.handle_transcript_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
    }
    app.handle_transcript_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    assert!(app.displayed_messages().contains("discussed in prose"));
    assert!(!app.displayed_messages().contains("message 19"));
    app.handle_transcript_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)).unwrap();
    assert_eq!(app.displayed_messages(), "src/example.rs\n-old\n+ÅNGSTRÖM");
    assert_eq!(app.composer.text(), "keep my draft");
}

fn hold_turn(app: &mut App) {
    let conversation = app.state.active_conversation().unwrap_or_default().to_owned();
    app.turns.insert(conversation, RunningTurn {
        task: tokio::spawn(std::future::pending()), input: None, interrupt: None, started: Instant::now(),
    });
}

#[tokio::test]
async fn resumed_running_turn_keeps_early_completion_and_unrelated_work() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("alpha").unwrap();
    app.state.submit().unwrap();
    app.state.observe_turn("alpha", "alpha-turn").unwrap();
    hold_turn(&mut app);
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("beta").unwrap();
    app.composer.insert_text("keep this draft");
    app.state.save_draft(&app.composer.text()).unwrap();
    let queued = app.state.queue_input("later").unwrap();
    let image = app.state.attach_image().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let image_path = file.path().to_owned();
    app.composer.attach_image(image, file);
    let draft = app.composer.text();
    let (client, server) = duplex(16384);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let peer = tokio::spawn(async move {
        let (reader, mut writer) = split(server);
        let mut lines = BufReader::new(reader).lines();
        let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "thread/resume");
        assert_eq!(request["params"]["threadId"], "beta");
        for message in [
            json!({"method":"turn/completed","params":{"threadId":"beta","turn":{"id":"older","status":"interrupted","items":[]}}}),
            json!({"method":"turn/completed","params":{"threadId":"beta","turn":{"id":"beta-turn","status":"completed","items":[{"id":"answer","type":"agentMessage","text":"Already finished"}]}}}),
            json!({"id":request["id"],"result":{"model":"fixture-model","reasoningEffort":"high","thread":{"id":"beta","turns":[{"id":"beta-turn","status":"inProgress","items":[]}]}}}),
        ] { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
        assert!(lines.next_line().await.unwrap().is_none());
    });
    tokio::time::timeout(Duration::from_secs(20), async {
        let resumed = codex::TurnSession::resume(connection.clone(), "beta").await.unwrap();
        app.attach_conversation(resumed);
        assert!(app.is_working());
        assert_eq!(app.state.active_turn(), "beta-turn");
        assert_eq!(app.state.phase(), NorthPhase::Dispatching);
        assert_eq!(app.state.prepare_queued_input().unwrap(), None);
        while app.turns.contains_key("beta") {
            app.collect_finished_turn().await;
            tokio::task::yield_now().await;
        }
        assert_eq!(app.state.phase(), NorthPhase::Completed);
        assert!(render_text(&mut app, 100, 25).contains("Already finished"));
        assert_eq!(app.composer.text(), draft);
        assert!(image_path.exists());
        assert_eq!(app.state.conversation("beta").unwrap().draft_attachments, vec![image]);
        assert_eq!(app.state.pending_inputs().iter().find(|input| input.number == queued).unwrap().status, "queued");
        assert_eq!(app.state.conversation("alpha").unwrap().active_turn, "alpha-turn");
        assert!(app.turns.contains_key("alpha"));
    }).await.unwrap();
    app.turns.remove("alpha").unwrap().task.abort();
    connection.close().await;
    peer.await.unwrap();
    driver.await.unwrap();
    drop(app);
    assert!(!image_path.exists());
}

#[tokio::test]
async fn switching_running_conversations_keeps_drafts_and_interrupts_only_the_selected_turn() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    for conversation in ["alpha", "beta"] {
        app.state.request_new_conversation().unwrap();
        app.state.settle_new_conversation(conversation).unwrap();
        app.state.observe_settings(conversation, "fixture-model", "high").unwrap();
    }
    app.switch_conversation("alpha").await;
    let (client, server) = duplex(16384);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let (reader, mut writer) = split(server);
    let mut lines = BufReader::new(reader).lines();
    let mut image_paths = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        for conversation in ["alpha", "beta"] {
            app.switch_conversation(conversation).await;
            app.composer.insert_text(&format!("start {conversation}"));
            app.handle_composer_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await;
            let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["method"], "turn/start");
            assert_eq!(request["params"]["threadId"], conversation);
            let turn = format!("{conversation}-turn");
            for message in [
                json!({"id":request["id"],"result":{"turn":{"id":turn}}}),
                json!({"method":"turn/started","params":{"threadId":conversation,"turn":{"id":turn}}}),
                json!({"method":"item/agentMessage/delta","params":{"threadId":conversation,"turnId":turn,"itemId":"progress","delta":format!("{conversation} is progressing")}}),
            ] { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
            while app.state.active_turn().is_empty() { app.collect_events(None); tokio::task::yield_now().await; }
            app.composer.insert_text(&format!("draft {conversation}"));
            let image = app.state.attach_image().unwrap();
            let file = tempfile::NamedTempFile::new().unwrap();
            image_paths.push(file.path().to_owned());
            app.composer.attach_image(image, file);
        }
        assert_eq!(app.turns.len(), 2);
        app.switch_conversation("alpha").await;
        assert!(app.composer.text().contains("draft alpha"));
        assert!(!app.composer.text().contains("draft beta"));
        let screen = render_text(&mut app, 110, 25);
        assert!(screen.contains("alpha is progressing"), "{screen}");
        assert!(!screen.contains("beta is progressing"), "{screen}");
        assert!(image_paths.iter().all(|path| path.exists()));
        app.interrupt_turn();
        let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], "turn/interrupt");
        assert_eq!(request["params"], json!({"threadId":"alpha","turnId":"alpha-turn"}));
        for message in [
            json!({"id":request["id"],"result":{}}),
            json!({"method":"turn/completed","params":{"threadId":"alpha","turn":{"id":"alpha-turn","status":"interrupted","items":[]}}}),
        ] { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
        while app.turns.contains_key("alpha") { app.collect_finished_turn().await; tokio::task::yield_now().await; }
        assert!(app.turns.contains_key("beta"));
        assert_eq!(app.state.conversation("beta").unwrap().phase, NorthPhase::Dispatching);
        assert!(!render_text(&mut app, 110, 25).contains("· failed"));
        app.switch_conversation("beta").await;
        assert!(app.composer.text().contains("draft beta"));
        assert!(app.is_working());
        let message = json!({"method":"turn/completed","params":{"threadId":"beta","turn":{"id":"beta-turn","status":"completed","items":[{"id":"answer","type":"agentMessage","text":"beta finished"}]}}});
        writer.write_all(format!("{message}\n").as_bytes()).await.unwrap();
        while !app.turns.is_empty() { app.collect_finished_turn().await; tokio::task::yield_now().await; }
        assert_eq!(app.state.phase(), NorthPhase::Completed);
        assert!(render_text(&mut app, 110, 25).contains("beta finished"));
        app.switch_conversation("alpha").await;
        assert_eq!(app.state.phase(), NorthPhase::Interrupted);
        assert!(!render_text(&mut app, 110, 25).contains("· failed"));
    }).await.unwrap();
    app.shutdown().await;
    connection.close().await;
    driver.await.unwrap();
    drop(app);
    assert!(image_paths.iter().all(|path| !path.exists()));
}

#[test]
fn tool_forms_use_checked_choices_validate_fields_and_keep_the_chat_draft() {
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread").unwrap();
    app.composer.insert_text("unsent chat draft");
    let message = json!({"id":72,"method":"mcpServer/elicitation/request","params":{
        "threadId":"thread","serverName":"fixture-tool","mode":"form","message":"Configure this request",
        "requestedSchema":{"type":"object","required":["a_count","b_choices","d_empty","e_enabled","f_kind"],"properties":{
            "a_count":{"type":"integer","minimum":1,"maximum":3},
            "b_choices":{"type":"array","items":{"anyOf":[{"const":"red","title":"Red"},{"const":"blue","title":"Blue"}]},"minItems":1,"maxItems":1},
            "c_note":{"type":"string"},"d_empty":{"type":"string","maxLength":4},
            "e_enabled":{"type":"boolean"},"f_kind":{"type":"string","enum":["raw-small","raw-large"],"enumNames":["Small","Large"]}
        }}
    }});
    app.state.observe_prompt(&prompts::decode_request(&message, &[]).unwrap().unwrap()).unwrap();
    let key = |app: &mut App, code| { app.handle_prompt_key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap(); };
    for character in "2.5".chars() { key(&mut app, KeyCode::Char(character)); }
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.state.active_prompt().unwrap().current, 1);
    for _ in 0..2 { key(&mut app, KeyCode::Backspace); }
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.state.active_prompt().unwrap().current, 2);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.state.active_prompt().unwrap().current, 2);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Enter);
    app.handle_prompt_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.state.active_prompt().unwrap().current, app.state.active_prompt().unwrap().questions.last().unwrap().number);
    key(&mut app, KeyCode::Enter);
    app.state.prepare_prompt_response().unwrap().unwrap();
    assert_eq!(prompts::response(app.state.active_prompt().unwrap()).unwrap(), json!({"id":72,"result":{
        "action":"accept","content":{"a_count":2,"b_choices":["red"],"d_empty":"","e_enabled":true,"f_kind":"raw-large"}
    }}));
    assert_eq!(app.composer.text(), "unsent chat draft");
}

#[tokio::test]
async fn unsupported_server_requests_receive_an_explicit_error() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let (reader, mut writer) = split(server);
    let mut lines = BufReader::new(reader).lines();
    writer.write_all(b"{\"id\":\"unknown-request\",\"method\":\"unsupported/request\",\"params\":{}}\n").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.request_errors.is_empty() { app.collect_events(None); tokio::task::yield_now().await; }
    }).await.unwrap();
    let reply: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
    assert_eq!(reply["id"], json!("unknown-request"));
    assert_eq!(reply["error"]["code"], json!(-32601));
    assert!(reply.get("result").is_none());
    app.shutdown().await;
    connection.close().await;
    driver.await.unwrap();
}

#[tokio::test]
async fn server_questions_use_the_prompt_editor_preserve_the_draft_and_reply_once() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread-prompt").unwrap();
    app.composer.insert_text("keep this draft");
    let image = app.state.attach_image().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    app.composer.attach_image(image, file);
    let original = app.composer.text();
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let (reader, mut writer) = split(server);
    let mut lines = BufReader::new(reader).lines();
    let question = json!({"id":"question-1","method":"item/tool/requestUserInput","params":{
        "threadId":"thread-prompt","turnId":"turn","itemId":"questions","questions":[
            {"id":"pick","header":"Approach","question":"How should we proceed?","options":[
                {"label":"Small","description":"Minimal changes"},{"label":"Complete","description":"Finish the feature"}]},
            {"id":"private","header":"Private note","question":"Add a private note","isSecret":true,"options":null},
        ]
    }});
    writer.write_all(format!("{question}\n").as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.state.active_prompt().is_none() { app.collect_events(None); tokio::task::yield_now().await; }
    }).await.unwrap();
    let screen = render_text(&mut app, 110, 25);
    assert!(screen.contains("How should we proceed?"), "{screen}");
    assert!(screen.contains("keep this draft"), "{screen}");
    app.handle_prompt_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).unwrap();
    app.handle_prompt_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    for character in "masked fixture".chars() {
        app.handle_prompt_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).unwrap();
    }
    let screen = render_text(&mut app, 110, 25);
    assert!(screen.contains("Add a private note"), "{screen}");
    assert!(screen.contains("••••••"), "{screen}");
    assert!(!screen.contains("masked fixture"), "{screen}");
    app.handle_prompt_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
    app.dispatch_prompt_response().unwrap();
    let response: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
    assert_eq!(response, json!({"id":"question-1","result":{"answers":{
        "pick":{"answers":["Complete"]},"private":{"answers":["masked fixture"]},
    }}}));
    assert_eq!(app.composer.text(), original);
    assert!(path.exists());
    let resolved = json!({"method":"serverRequest/resolved","params":{"threadId":"thread-prompt","requestId":"question-1"}});
    writer.write_all(format!("{resolved}\n").as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.state.active_prompt().is_some() {
            app.collect_events(None); app.collect_prompt_responses().await; tokio::task::yield_now().await;
        }
    }).await.unwrap();
    app.collect_prompt_responses().await;
    app.dispatch_prompt_response().unwrap();
    assert!(app.prompt_responses.is_empty());
    assert_eq!(app.composer.text(), original);
    assert!(app.prompt_editor.lines().join("\n").is_empty());
    assert_eq!(app.state.submit().unwrap(), vec![image]);
    assert!(path.exists());
    connection.close().await;
    driver.await.unwrap();
}

#[tokio::test]
async fn steering_keys_send_the_active_turn_and_keep_rejected_images_editable() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread-steer").unwrap();
    app.state.submit().unwrap();
    hold_turn(&mut app);
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let (reader, mut writer) = split(server);
    let mut lines = BufReader::new(reader).lines();
    let started = json!({"method":"turn/started","params":{"threadId":"thread-steer","turn":{"id":"turn-steer"}}});
    writer.write_all(format!("{started}\n").as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.state.active_turn().is_empty() {
            app.collect_events(None); tokio::task::yield_now().await;
        }
    }).await.unwrap();
    app.composer.insert_text("focus on this image");
    let attachment = app.state.attach_image().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    app.composer.attach_image(attachment, file);
    app.handle_composer_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await;
    let number = app.state.pending_inputs()[0].number;
    assert!(app.composer.is_empty());
    app.dispatch_pending_input().unwrap();
    let request: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
    assert_eq!(request["method"], "turn/steer");
    assert_eq!(request["params"]["expectedTurnId"], "turn-steer");
    assert_eq!(request["params"]["threadId"], "thread-steer");
    assert_eq!(request["params"]["input"][1], json!({"type":"localImage","path":path}));
    assert!(path.exists());
    app.composer.insert_text("keep my newer draft");
    let rejected = json!({"id":request["id"],"error":{"code":-32600,"message":"turn already ended"}});
    writer.write_all(format!("{rejected}\n").as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !app.steering.is_empty() { app.collect_steering().await; tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(app.state.pending_inputs()[0].status, "not sent");
    assert!(path.exists());
    assert_eq!(app.composer.text(), "keep my newer draft");
    let screen = render_text(&mut app, 110, 24);
    assert!(screen.contains("focus on this image"), "{screen}");
    assert_eq!(app.state.pending_inputs()[0].number, number);
    app.edit_pending_input().unwrap();
    assert_eq!(app.composer.text(), "keep my newer draft");
    app.composer.replace_text("");
    app.edit_pending_input().unwrap();
    assert!(app.state.pending_inputs().is_empty());
    assert!(app.composer.text().contains("focus on this image"));
    let restored = app.composer.take_submission();
    assert_eq!(restored.image_paths(&[attachment]).unwrap(), vec![path.clone()]);
    assert!(path.exists());
    drop(restored);
    assert!(!path.exists());
    app.shutdown().await;
    connection.close().await;
    driver.await.unwrap();
}

#[tokio::test]
async fn queued_key_dispatches_after_completion_without_consuming_the_newer_draft() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex, split};
    use serde_json::{Value, json};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread-queue").unwrap();
    app.state.submit().unwrap();
    app.composer.insert_text("queued image task");
    let attachment = app.state.attach_image().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    app.composer.attach_image(attachment, file);
    assert!(!navigate_view(&mut app.state, &KeyCode::Tab, false).unwrap());
    let second_attachment = app.state.attach_image().unwrap();
    let second_file = tempfile::NamedTempFile::new().unwrap();
    let second_path = second_file.path().to_owned();
    app.composer.attach_image(second_attachment, second_file);
    app.handle_composer_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)).await;
    let number = app.state.pending_inputs()[0].number;
    let queued_text = app.state.pending_inputs()[0].text.clone();
    app.dispatch_pending_input().unwrap();
    assert!(app.turns.is_empty());
    assert_eq!(app.state.pending_inputs()[0].status, "queued");
    app.composer.insert_text("my unsent draft");
    let newer_image = app.state.attach_image().unwrap();
    let newer_file = tempfile::NamedTempFile::new().unwrap();
    let newer_path = newer_file.path().to_owned();
    app.composer.attach_image(newer_image, newer_file);
    let draft = app.composer.text();
    app.state.settle_success().unwrap();
    let (client, server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.connection = Some(connection.clone());
    app.events = Some(events);
    let (reader, mut writer) = split(server);
    let mut lines = BufReader::new(reader).lines();
    app.dispatch_pending_input().unwrap();
    let request: Value = serde_json::from_str(&tokio::time::timeout(Duration::from_secs(5), lines.next_line()).await.unwrap().unwrap().unwrap()).unwrap();
    assert_eq!(request["method"], "turn/start");
    assert_eq!(request["params"]["input"].as_array().unwrap().len(), 3);
    assert_eq!(request["params"]["input"][0]["type"], "text");
    assert_eq!(request["params"]["input"][0]["text"], queued_text);
    assert_eq!(request["params"]["input"][1], json!({"type":"localImage","path":path}));
    assert_eq!(request["params"]["input"][2], json!({"type":"localImage","path":second_path}));
    assert!(path.exists());
    assert!(second_path.exists());
    assert_eq!(app.composer.text(), draft);
    let messages = [
        json!({"id":request["id"],"result":{"turn":{"id":"turn-queued"}}}),
        json!({"method":"turn/started","params":{"threadId":"thread-queue","turn":{"id":"turn-queued"}}}),
        json!({"method":"turn/completed","params":{"threadId":"thread-queue","turn":{"id":"turn-queued","status":"completed","items":[{"type":"agentMessage","id":"reply","text":"queued result"}]}}}),
    ];
    for message in messages { writer.write_all(format!("{message}\n").as_bytes()).await.unwrap(); }
    tokio::time::timeout(Duration::from_secs(5), async {
        while !app.turns.is_empty() {
            app.collect_events(None); app.collect_finished_turn().await; tokio::task::yield_now().await;
        }
    }).await.unwrap();
    assert!(app.state.pending_inputs().is_empty());
    assert!(!app.pending_images.contains_key(&number));
    assert!(!path.exists());
    assert!(!second_path.exists());
    assert_eq!(app.composer.text(), draft);
    assert_eq!(app.state.submit().unwrap(), vec![newer_image]);
    assert!(newer_path.exists());
    let screen = render_text(&mut app, 110, 24);
    assert!(screen.contains("queued image task"), "{screen}");
    assert!(screen.contains("queued result"), "{screen}");
    app.shutdown().await;
    connection.close().await;
    driver.await.unwrap();
}

#[tokio::test]
async fn streamed_text_and_command_output_render_before_the_turn_finishes() {
    use tokio::io::{AsyncWriteExt, duplex, split};
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::ephemeral(directory.path().to_owned()).unwrap();
    app.state.request_new_conversation().unwrap();
    app.state.settle_new_conversation("thread-live").unwrap();
    app.record_chat(Speaker::Operator, "show live progress".into());
    app.state.submit().unwrap();
    hold_turn(&mut app);
    let (client, mut server) = duplex(8192);
    let (reader, writer) = split(client);
    let (connection, events, driver) = rpc::Rpc::start(reader, writer);
    app.events = Some(events);
    app.connection = Some(connection.clone());
    let notifications = [
        serde_json::json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-live","turnId":"turn-live","itemId":"message","delta":"Inspecting the files"}}),
        serde_json::json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"thread-live","turnId":"turn-live","itemId":"command","delta":"first output line\nsecond output line"}}),
        serde_json::json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-live","turnId":"turn-live","itemId":"message","delta":" before completion"}}),
    ];
    for message in notifications {
        server.write_all(format!("{message}\n").as_bytes()).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.events.as_ref().unwrap().len() < 3 { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert!(app.collect_events(Some(Duration::ZERO)));
    assert_eq!(app.events.as_ref().unwrap().len(), 2);
    assert!(navigate_view(&mut app.state, &KeyCode::Tab, true).unwrap());
    assert_eq!(app.state.active_view(), "goals");
    app.composer.insert_text("draft between notifications");
    assert!(navigate_view(&mut app.state, &KeyCode::Tab, true).unwrap());
    app.collect_events(None);
    assert!(app.events.as_ref().unwrap().is_empty());
    assert_eq!(app.composer.text(), "draft between notifications");
    let screen = render_text(&mut app, 100, 24);
    assert!(screen.contains("Inspecting the files before completion"), "{screen}");
    assert!(screen.contains("second output line"), "{screen}");
    assert!(app.is_working());
    let completed = serde_json::json!({"method":"item/completed","params":{"threadId":"thread-live","turnId":"turn-live","item":{"id":"message","type":"agentMessage","text":"Inspection complete"}}});
    server.write_all(format!("{completed}\n").as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.events.as_ref().unwrap().is_empty() { tokio::task::yield_now().await; }
    }).await.unwrap();
    app.collect_events(None);
    let screen = render_text(&mut app, 100, 24);
    assert_eq!(screen.matches("Inspection complete").count(), 1);
    assert!(!screen.contains("Inspecting the files"));
    for item in [
        serde_json::json!({"id":"image-input","type":"userMessage","content":[{"type":"text","text":"image payload"},{"type":"localImage","path":"/recorded/image.png"}]}),
        serde_json::json!({"id":"patch","type":"fileChange","status":"completed","changes":[{"path":"src/Ångström.rs","diff":"-old\n+new"}]}),
    ] {
        let event = serde_json::json!({"method":"item/completed","params":{"threadId":"thread-live","turnId":"turn-live","item":item}});
        server.write_all(format!("{event}\n").as_bytes()).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.events.as_ref().unwrap().len() < 2 { tokio::task::yield_now().await; }
    }).await.unwrap();
    app.collect_events(None);
    let image = app.state.chat().iter().find(|entry| entry.key == "image-input").unwrap();
    assert!(image.text.starts_with("image payload"));
    assert!(image.text.contains('1'));
    assert!(!image.text.contains("/recorded/image.png"));
    let patch = app.state.chat().iter().find(|entry| entry.key == "patch").unwrap();
    assert_eq!(patch.kind, "fileChange");
    assert_eq!(patch.text, "src/Ångström.rs\n-old\n+new");
    app.state.toggle_changes().unwrap();
    let screen = render_text(&mut app, 100, 24);
    for payload in ["src/Ångström.rs", "-old", "+new"] { assert!(screen.contains(payload), "{screen}"); }
    app.shutdown().await;
    connection.close().await;
    driver.await.unwrap();
}
use ratatui::backend::TestBackend;

fn render_text(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered.push_str(buffer[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    rendered
}

fn accepted_frame_app() -> App {
    let mut app = App::ephemeral(PathBuf::from("/tmp/demo")).unwrap();
    app.branch = "north-usable-tui".into();
    app.model = "gpt-example".into();
    app.reasoning_effort = "high".into();
    app.composer.insert_text("next question");
    app.transcript = vec![
        (Speaker::Operator, "FIRST".into()),
        (Speaker::North, "first answer".into()),
        (Speaker::System, "visible diagnostic".into()),
    ];
    app.status = "complete".into();
    app
}

#[test]
fn cached_transcript_shows_errors_that_could_not_be_saved() {
    let mut app = accepted_frame_app();
    assert!(render_text(&mut app, 100, 24).contains("first answer"));
    assert!(app.rendered_transcript.is_some());
    app.append_transcript_error("Could not save this update".into());
    assert!(render_text(&mut app, 100, 24).contains("Could not save this update"));
}

#[test]
fn cached_transcript_viewport_tracks_pending_input_scroll_and_resize() {
    let mut app = accepted_frame_app();
    app.transcript = (0..50).map(|index| (Speaker::North, format!("message {index}"))).collect();
    assert!(render_text(&mut app, 100, 24).contains("message 49"));
    app.state.queue_input("waiting input").unwrap();
    assert!(render_text(&mut app, 100, 24).contains("waiting input"));
    app.state.scroll_transcript(12.0).unwrap();
    let scrolled = render_text(&mut app, 100, 24);
    assert!(!scrolled.contains("waiting input"));
    app.rendered_transcript_viewport = None;
    assert_eq!(render_text(&mut app, 100, 24), scrolled);
    let resized = render_text(&mut app, 70, 18);
    app.rendered_transcript_viewport = None;
    assert_eq!(render_text(&mut app, 70, 18), resized);
}

fn submission(text: &str) -> Submission {
    let mut composer = Composer::new();
    composer.insert_text(text);
    composer.take_submission()
}

#[tokio::test]
async fn state_drives_the_tui_goal_create_inspect_edit_inspect_journey() {
    let mut app = App::ephemeral(PathBuf::from("/tmp/demo")).unwrap();

    assert!(!app.accept_submission(submission("/goal")).await);
    assert!(!app.accept_submission(submission("Build North")).await);
    assert!(
        !app.accept_submission(submission("Make Galileo own Goals"))
            .await
    );

    assert!(!app.accept_submission(submission("/goals")).await);
    let created = render_text(&mut app, 100, 18);
    assert!(created.contains("Build North"));
    assert_eq!(app.state.active_goal().unwrap().order(), 1);
    assert_eq!(app.state.active_goal().unwrap().status(), "active");
    assert!(created.contains("Make Galileo own Goals"));

    assert!(!app.accept_submission(submission("/goal edit")).await);
    assert_eq!(app.composer.text(), "Make Galileo own Goals");
    assert!(
        !app.accept_submission(submission("Make Galileo own the whole TUI"))
            .await
    );
    let edited = render_text(&mut app, 100, 20);
    assert!(edited.contains("Make Galileo own the whole TUI"));
    assert_eq!(app.state.active_goal().unwrap().prior_objectives(), ["Make Galileo own Goals"]);
    assert!(edited.contains("Make Galileo own Goals"));
}

#[tokio::test]
async fn inline_goal_commands_edit_and_clear_without_submitting_a_turn() {
    let mut app = App::ephemeral(PathBuf::from("/tmp/demo")).unwrap();
    app.accept_submission(submission("/goal edit")).await;

    app.accept_submission(submission("  /goal\tship 世界  ")).await;
    assert_eq!(app.state.active_goal().unwrap().objective(), "ship 世界");
    app.accept_submission(submission("/goal edit ship the repaired harness")).await;
    let goal = app.state.active_goal().unwrap();
    assert_eq!(goal.objective(), "ship the repaired harness");
    assert_eq!(goal.prior_objectives(), ["ship 世界"]);

    app.accept_submission(submission("/goal clear extra")).await;
    assert!(app.state.active_goal().is_some());
    app.accept_submission(submission("/goal clear")).await;
    assert!(app.state.active_goal().is_none());
    assert_eq!(app.state.goals()[0].status(), "cleared");
    app.accept_submission(submission("/goal set second outcome")).await;
    assert_eq!(app.state.active_goal().unwrap().objective(), "second outcome");
    assert!(!app.is_working());
    assert!(app.state.host_effect().is_none());
}

#[test]
fn source_resolves_unknown_commands_without_dispatching_chat() {
    let mut state = NorthState::open().unwrap();
    state.accept_input("/unknown argument").unwrap();
    assert!(state.input_is_command());
    assert!(state.host_effect().is_none());
}

#[test]
fn tab_and_arrow_keys_navigate_the_two_product_views() {
    let mut state = NorthState::open().expect("North Galileo program opens");
    for (key, expected) in [
        (KeyCode::Tab, "goals"), (KeyCode::Right, "chat"),
        (KeyCode::Tab, "goals"), (KeyCode::Left, "chat"),
        (KeyCode::BackTab, "goals"), (KeyCode::Esc, "chat"),
    ] {
        assert!(navigate_view(&mut state, &key, true).unwrap());
        assert_eq!(state.active_view(), expected);
    }
    assert!(!navigate_view(&mut state, &KeyCode::Up, true).unwrap());
    assert!(!navigate_view(&mut state, &KeyCode::Left, false).unwrap());
}

#[test]
fn conversation_picker_and_replay_replace_the_prior_thread_completely() {
    let mut app = accepted_frame_app();
    app.state.request_new_conversation().unwrap();
    assert_eq!(app.state.conversation_change(), north_state::ConversationChange::Opening);
    app.state.settle_new_conversation("thread-first").unwrap();
    assert_eq!(app.state.active_conversation(), Some("thread-first"));
    assert!(app.state.request_switch_conversation("thread-next").is_err());
    assert_eq!(app.state.conversation_change(), north_state::ConversationChange::Ready);
    assert_eq!(app.state.active_conversation(), Some("thread-first"));
    let rendered = render_text(&mut app, 110, 12);

    assert!(rendered.contains(&app.model));
    assert!(rendered.contains(&app.reasoning_effort));
    assert!(rendered.contains(&app.cwd.display().to_string()));
    assert!(rendered.contains("next question"));
    assert!(rendered.contains("FIRST"));
    assert!(rendered.contains("first answer"));
    assert!(rendered.contains("visible diagnostic"));

    app.state.open_history(&[codex::ConversationOption {
        id: "thread-next".into(),
        title: "Galileo moat".into(),
        preview: "Drive the thesis".into(),
        current: false,
    }], "", false).unwrap();
    let picker = render_text(&mut app, 100, 18);
    assert_eq!(app.state.menu().kind, "history");
    assert!(picker.contains("Galileo moat"));
    assert!(picker.contains("Drive the thesis"));

    app.state.close_menu().unwrap();
    app.state.observe_conversation("thread-next").unwrap();
    app.state.observe_conversation("thread-third").unwrap();
    app.state.request_switch_conversation("thread-next").unwrap();
    assert_eq!(app.state.conversation_change(), north_state::ConversationChange::Switching);
    app.state.fail_switch_conversation("thread-next").unwrap();
    assert_eq!(app.state.conversation_change(), north_state::ConversationChange::Ready);
    assert_eq!(app.state.active_conversation(), Some("thread-first"));
    app.state.request_switch_conversation("thread-next").unwrap();
    app.state.settle_switch_conversation("thread-next").unwrap();
    assert_eq!(app.state.active_conversation(), Some("thread-next"));
    app.load_conversation(codex::ConversationSnapshot {
        id: "thread-next".into(),
        model: "gpt-5.6-terra".into(),
        reasoning_effort: "high".into(),
        turns: vec![],
        accepted_inputs: vec![],
        entries: [
            ("user", "userMessage", "new thread prompt"),
            ("comment", "agentMessage", "checking the project"),
            ("cmd", "commandExecution", "cargo test\n12 passed"),
            ("diff", "fileChange", "src/main.rs\n-before\n+after"),
            ("answer", "agentMessage", "new thread answer"),
        ].into_iter().map(|(key, kind, text)| codex::ChatUpdate {
            conversation: "thread-next".into(), turn: "turn-next".into(), key: key.into(), kind: kind.into(), text: text.into(), status: "completed".into(), append: false,
        }).collect(),
    });
    let replay = render_text(&mut app, 100, 30);
    assert!(replay.contains("new thread prompt"));
    assert!(replay.contains("cargo test"));
    assert!(replay.contains("new thread answer"));
    assert!(replay.contains("12 passed"));
    assert!(!replay.contains("FIRST"));
    assert!(!replay.contains("first answer"));
    assert_eq!(app.model, "gpt-5.6-terra");
    assert_eq!(app.reasoning_effort, "high");
    app.state.observe_chat_item(&north_state::ChatEntryInput {
        conversation: "thread-next", turn: "turn-next", key: "cmd", kind: "commandExecution", text: "cargo test\n13 passed", status: "completed", append: false,
    }).unwrap();
    app.project_chat();
    assert_eq!(app.state.chat().iter().filter(|entry| entry.key == "cmd").count(), 1);
    app.state.toggle_changes().unwrap();
    app.project_chat();
    assert_eq!(app.displayed_messages(), "src/main.rs\n-before\n+after");
}

#[tokio::test]
async fn history_search_navigation_and_cancel_keep_the_composer() {
    let mut app = accepted_frame_app();
    app.composer.replace_text("unsent draft");
    let candidates = [
        ("alpha", "Roadmap", "First plan"),
        ("beta", "ÅNGSTRÖM", "Measurements"),
        ("gamma", "More measurements", "ångström notes"),
    ].into_iter().map(|(id, title, preview)| codex::ConversationOption {
        id: id.into(), title: title.into(), preview: preview.into(), current: false,
    }).collect::<Vec<_>>();
    app.state.open_history(&candidates, "", false).unwrap();
    for character in "ångström".chars() {
        app.handle_menu_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)).await.unwrap();
    }
    assert_eq!(app.state.menu().query, "ångström");
    assert_eq!(app.state.menu().rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(), ["beta", "gamma"]);
    let screen = render_text(&mut app, 100, 24);
    assert!(screen.contains("ÅNGSTRÖM"));
    assert!(!screen.contains("Roadmap"));
    app.handle_menu_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)).await.unwrap();
    assert_eq!(app.state.menu().selection, 1);
    app.state.accept_menu().unwrap();
    assert_eq!(app.state.host_effect().unwrap().payload(), "gamma");
    app.state.clear_host_effect().unwrap();
    app.state.open_history(&candidates, "unmatched", false).unwrap();
    app.state.accept_menu().unwrap();
    assert!(app.state.host_effect().is_none());
    assert!(app.state.menu().rows.is_empty());
    assert!(!render_text(&mut app, 100, 24).contains("ÅNGSTRÖM"));
    app.handle_menu_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).await.unwrap();
    assert_eq!(app.composer.text(), "unsent draft");
    assert!(app.state.menu().kind.is_empty());
}

#[test]
fn model_and_effort_picker_screens_render_catalog_values() {
    let models = vec![
        codex::ModelOption {
            model: "gpt-5.6-sol".into(),
            description: "Latest frontier agentic coding model.".into(),
            reasoning: vec![
                codex::ReasoningOption {
                    effort: "low".into(),
                    description: "Fast responses with lighter reasoning".into(),
                },
                codex::ReasoningOption {
                    effort: "high".into(),
                    description: "Greater reasoning depth for complex problems".into(),
                },
                codex::ReasoningOption {
                    effort: "max".into(),
                    description: "For difficult problems".into(),
                },
            ],
            default_effort: "low".into(),
            is_default: true,
        },
        codex::ModelOption {
            model: "gpt-5.6-terra".into(),
            description: "Balanced agentic coding model for everyday work.".into(),
            reasoning: vec![],
            default_effort: "medium".into(),
            is_default: false,
        },
    ];
    let mut app = accepted_frame_app();
    app.model = "gpt-5.6-sol".into();
    app.reasoning_effort = "high".into();
    app.picker = Picker::models(models.clone(), &app.model);

    let model_view = render_text(&mut app, 110, 18);
    assert!(model_view.contains("gpt-5.6-sol"));
    assert_eq!(app.model, "gpt-5.6-sol");
    assert!(matches!(&app.picker, Some(Picker::Models { index: 0, .. })));
    assert!(model_view.contains("gpt-5.6-terra"));

    app.picker = Some(Picker::efforts(
        models,
        0,
        &app.model,
        &app.reasoning_effort,
        true,
    ));
    let effort_view = render_text(&mut app, 110, 18);
    assert!(effort_view.to_lowercase().contains(&app.reasoning_effort));
    assert_eq!(app.reasoning_effort, "high");
    assert!(matches!(&app.picker, Some(Picker::Efforts { index: 1, advanced, .. }) if advanced.iter().any(|option| option.effort == "max")));
    assert!(effort_view.to_lowercase().contains("max"));
}

#[test]
fn switchboard_shows_each_resolved_unit_once() {
    let mut app = accepted_frame_app();
    app.picker = Some(Picker::switchboard(vec![
        agent_catalog::ActivationUnit {
            id: "worktree-guard".into(),
            kind: "hook".into(),
            active: true,
            description: "Protect worktrees".into(),
            source: PathBuf::from("/tmp/worktree-guard.sh"),
        },
        agent_catalog::ActivationUnit {
            id: "planning".into(),
            kind: "module".into(),
            active: false,
            description: "Planning".into(),
            source: PathBuf::from("/tmp/catalog.json"),
        },
        reference_unit("agent-policy", "Agent Policy"),
    ]));

    let rendered = render_text(&mut app, 100, 18);
    assert!(matches!(&app.picker, Some(Picker::Switchboard { units, .. }) if units.iter().any(|unit| unit.kind == "hook") && units.iter().any(|unit| unit.kind == "module")));
    let compact = rendered
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(compact.contains("worktree-guard"));
    assert!(compact.contains("planning"));
    assert!(compact.contains("agent-policy"));
    assert!(!rendered.contains("Agent Policy"));
    for id in ["worktree-guard", "planning", "agent-policy"] {
        assert_eq!(rendered.matches(id).count(), 1);
    }
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
    terminal.draw(|frame| render_application(frame, &mut app, false)).unwrap();
    for (id, color) in [("worktree-guard", Color::Green), ("planning", Color::DarkGray), ("agent-policy", Color::Gray)] {
        let (y, row) = rendered.lines().enumerate().find(|(_, row)| row.contains(id)).unwrap();
        let x = row[..row.find(id).unwrap()].chars().count();
        assert_eq!(terminal.backend().buffer()[(x as u16, y as u16)].fg, color);
    }
}

#[test]
fn settings_menu_reopening_restores_all_offered_rows_and_positions() {
    let mut app = App::ephemeral(PathBuf::from("/tmp/north-settings-menu-test")).unwrap();
    let labels = (0..102).map(|index| format!("unit-{index}")).collect::<Vec<_>>();
    let rows = labels.iter().map(|label| (label.as_str(), label.as_str(), "Description", "on")).collect::<Vec<_>>();
    app.state.open_settings_menu("config", "Context Switchboard", &rows).unwrap();
    assert!(render_text(&mut app, 100, 24).contains("unit-0"));
    app.state.query_menu("unit-9").unwrap();
    app.state.open_settings_menu("config", "Context Switchboard", &rows).unwrap();
    assert!(app.state.menu().query.is_empty());
    assert_eq!(app.state.menu().rows.iter().map(|row| &row.key).collect::<Vec<_>>(), labels.iter().collect::<Vec<_>>());
    for (index, row) in app.state.menu().rows.iter().enumerate() {
        assert_eq!(row.key, labels[index]);
        let index = i64::try_from(index).unwrap();
        assert_eq!(row.position, index);
    }
}

fn reference_unit(id: &str, description: &str) -> agent_catalog::ActivationUnit {
    agent_catalog::ActivationUnit {
        id: id.into(),
        kind: "skill".into(),
        active: true,
        description: description.into(),
        source: PathBuf::from(format!("/tmp/skills/{id}/SKILL.md")),
    }
}

#[test]
fn switchboard_reference_keeps_the_draft_and_inserts_the_owning_source() {
    let mut app = accepted_frame_app();
    app.picker = Some(Picker::switchboard(vec![reference_unit(
        "agent-policy",
        "Author agent policy",
    )]));
    app.reference_switchboard_selection();
    assert!(app.picker.is_none());
    assert!(app.composer.text().starts_with("next question\n@agent-policy"));
    assert!(app.composer.text().contains("/tmp/skills/agent-policy/SKILL.md"));
    assert!(app.composer.text().ends_with(char::is_whitespace));
    assert!(!app.is_working());
}

#[test]
fn reference_menu_filters_renders_and_inserts_without_submitting() {
    let mut app = accepted_frame_app();
    app.composer.replace_text("Please use @policy");
    app.reference_candidates = Some(vec![
        reference_unit("agent-policy", "Author agent policy"),
        reference_unit("agent-policy-audit", "Detailed policy notes"),
        reference_unit("threejs-animation", "Animate objects"),
    ].into_iter().map(Into::into).collect());
    app.refresh_reference_menu();
    let rendered = render_text(&mut app, 130, 22);
    for expected in ["Author agent policy", "agent-policy-audit"] {
        assert!(rendered.contains(expected), "missing {expected}");
    }
    assert!(!rendered.contains("threejs-animation"));
    assert!(
        app.handle_reference_key(&KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
    );
    assert_eq!(app.state.reference_selection(), 1);
    assert!(
        app.handle_reference_key(&KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL))
    );
    assert_eq!(app.state.reference_selection(), 0);
    assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(app.composer.text().starts_with("Please use @agent-policy-audit"));
    assert!(app.composer.text().contains("/tmp/skills/agent-policy-audit/SKILL.md"));
    assert!(app.composer.text().ends_with(char::is_whitespace));
    assert!(!app.is_working());
}

#[test]
fn escape_closes_references_and_typing_reopens_them() {
    let mut app = accepted_frame_app();
    app.composer.replace_text("@policy");
    app.reference_candidates = Some(vec![reference_unit("agent-policy", "Policy").into()]);
    app.refresh_reference_menu();
    assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(app.reference_query().is_none());
    assert_eq!(app.composer.text(), "@policy");
    app.composer.insert_text("-d");
    app.refresh_reference_menu();
    assert_eq!(app.reference_query().as_deref(), Some("policy-d"));
}

#[test]
fn project_reference_filters_and_inserts_the_exact_path_without_submitting() {
    let mut app = accepted_frame_app();
    app.composer.replace_text("Inspect @ång");
    app.reference_candidates = Some(vec![
        references::Reference {name: "src/Ångström notes.rs".into(), description: "Project file".into(), kind: "file".into(), path: "/tmp/project/src/Ångström notes.rs".into()},
        reference_unit("agent-policy", "Policy").into(),
    ]);
    app.refresh_reference_menu();
    let rendered = render_text(&mut app, 130, 22);
    assert!(rendered.contains("src/Ångström notes.rs"), "{rendered}");
    assert_eq!(app.state.references()[0].kind, "file");
    assert!(!rendered.contains("agent-policy"));
    assert!(app.handle_reference_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert!(app.composer.text().starts_with("Inspect @src/Ångström notes.rs"));
    assert!(app.composer.text().contains("/tmp/project/src/Ångström notes.rs"));
    assert!(app.composer.text().ends_with(char::is_whitespace));
    assert!(!app.is_working());
    assert!(app.reference_query().is_none());
}

#[test]
fn editor_receives_the_source_as_one_literal_argument() {
    let path = Path::new("/tmp/a skill; $(printf unexpected)/SKILL.md");
    let output = editor_command("printf '%s\\n'", path).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", path.display())
    );
}

#[test]
fn transcript_keeps_the_newest_turn_visible() {
    let mut app = accepted_frame_app();
    app.transcript = (0..20)
        .map(|index| (Speaker::North, format!("answer {index}")))
        .collect();

    let rendered = render_text(&mut app, 80, 10);
    assert!(rendered.contains("answer 19"));
    assert!(!rendered.contains("answer 0 "));
}

#[test]
fn markdown_transcript_preserves_payloads_and_applies_inline_styles() {
    let mut app = accepted_frame_app();
    app.transcript = vec![(
        Speaker::North,
        concat!(
            "---\ntitle: Complete fixture\n---\n\n",
            "# Heading {#heading}\n\n",
            "**bold** *emphasis* ~~deleted~~ `inline code`  \n",
            "hard break and <https://openai.com>.\n\n",
            "- plain item\n- [x] completed task\n- [ ] pending task\n\n",
            "1. ordered item\n2. second item\n\n",
            "> quoted text\n\n",
            "---\n\n",
            "```rust\nfn main() {}\n```\n\n",
            "[OpenAI](https://openai.com) and ![diagram](diagram.png)\n\n",
            "Inline <kbd>HTML</kbd> remains visible.\n\n",
            "| A | B |\n|:--|--:|\n| 1 | 2 |\n\n",
            "Term\n: definition text\n\n",
            "Footnote reference[^note].\n\n[^note]: Footnote body.\n\n",
            "Inline math $x + y$, display math $$z = 3$$, H ~2~ O, and x ^2^."
        )
        .into(),
    )];

    let text = conversation_text(&app, 100);
    let rendered = text.to_string();
    for expected in [
        "Heading",
        "bold",
        "emphasis",
        "deleted",
        "inline code",
        "hard break",
        "- [x] completed task",
        "1. ordered item",
        "quoted text",
        "fn main() {}",
        "OpenAI",
        "https://openai.com",
        "[img] diagram (diagram.png)",
        "<kbd>HTML</kbd>",
        "definition text",
        "Footnote body",
        "$x + y$",
        "$$z = 3$$",
    ] {
        assert!(
            rendered.contains(expected),
            "missing rendered markdown: {expected}"
        );
    }
    assert!(rendered.contains('┌'));
    for source_marker in ["**bold**", "~~deleted~~", "`inline code`", "![diagram]"] {
        assert!(!rendered.contains(source_marker));
    }
    assert!(text.lines.iter().flat_map(|line| &line.spans).any(|span| {
        span.content.contains("bold") && span.style.add_modifier.contains(Modifier::BOLD)
    }));
}

#[tokio::test]
async fn working_row_follows_the_submitted_message_inside_the_transcript() {
    let mut app = accepted_frame_app();
    app.transcript = vec![(Speaker::Operator, "current question".into())];
    app.composer = Composer::new();
    app.composer.insert_text("editable next draft");
    hold_turn(&mut app);

    let rendered = render_text(&mut app, 80, 12);
    let rows = rendered.lines().collect::<Vec<_>>();
    let message_row = rows
        .iter()
        .position(|row| row.contains("current question"))
        .unwrap();
    let working_row = rows
        .iter()
        .position(|row| row.contains("0s"))
        .unwrap();
    let composer_row = rows.iter().position(|row| row.contains("editable next draft")).unwrap();

    assert_eq!(working_row, message_row + 2);
    assert!(working_row < composer_row);
    app.shutdown().await;
}

