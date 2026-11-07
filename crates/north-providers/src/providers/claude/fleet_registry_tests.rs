use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::fleet::registry::*;
use crate::fleet::claims::{self};
use crate::fleet::model::EntryKind;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::test_support::{Internals, WINDOW_SIZE};
use crate::fleet::fakes::Paper;
use super::Claude;

fn fleet(home: &Path, profiles: &str) -> Fleet {
    fleet_claiming(home, profiles, Arc::new(claims::Held::default()))
}

fn fleet_claiming(home: &Path, profiles: &str, held: Arc<claims::Held>) -> Fleet {
    let local = home.join(".local/state/north/profiles/claude-a");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join(".credentials.json"), "{}").unwrap();
    fs::write(local.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"local-login"}}"#).unwrap();
    let policy = crate::platform::orchestration::under(home);
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    let here = crate::platform::host::name();
    fs::write(&policy, format!("[accounts.claude.profiles.a]\n\"{here}\" = {{ path = \"~/.local/state/north/profiles/claude-a\" }}\n{profiles}")).unwrap();
    Fleet::at_claiming(home.into(), Runtimes::new(vec![Arc::new(Claude)]), Arc::new(Paper::default()), held).unwrap()
}

#[test]
fn a_revived_session_shows_the_turns_and_final_report_after_a_frame_larger_than_a_read_window() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let session = fleet.root().join("revived");
    let record = serde_json::json!({"name": "revived", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "c", "host_dir": session, "stream_offset": 0, "machine": {"name": "far", "ssh": "tom@far"}});
    fs::create_dir_all(session.join("g1")).unwrap();
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let assistant = |text: &str| serde_json::json!({"type": "assistant", "session_id": "c", "message": {"content": [{"type": "text", "text": text}]}}).to_string();
    let strip = serde_json::json!({"type": "user", "session_id": "c", "message": {"content": [{"type": "tool_result", "content": "A".repeat(3 * WINDOW_SIZE as usize)}]}}).to_string();
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": "c"}).to_string();
    let lines = [assistant("before the revive"), result.clone(), strip, assistant("final report"), result];
    fs::write(session.join("g1/stream.jsonl"), lines.map(|line| line + "\n").concat()).unwrap();

    let follower = fleet.follow("revived").unwrap();

    let replies: Vec<_> = follower.transcript().entries.iter().filter(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.as_str()).collect();
    assert_eq!(replies, ["before the revive", "final report"]);
}

#[test]
fn an_unreadable_registry_is_not_an_empty_fleet() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fs::remove_dir_all(fleet.root()).unwrap();

    assert_eq!(fleet.live_owners(), None);
}


struct Window {
    inbox: std::os::unix::net::UnixListener,
    token: String,
}

fn open_window(home: &Path, pid: u32, conversation: &str, started: u64) -> Window {
    let sessions = home.join(".local/state/north/profiles/claude-a/sessions");
    fs::create_dir_all(&sessions).unwrap();
    let socket = home.join(format!("{pid}.sock"));
    let token = format!("{pid:032x}");
    let record = serde_json::json!({"pid": pid, "sessionId": conversation, "kind": "interactive", "entrypoint": "cli", "startedAt": started, "cwd": "/tmp", "messagingSocketPath": socket});
    fs::write(sessions.join(format!("{pid}.json")), record.to_string()).unwrap();
    fs::write(sessions.join(format!("{pid}.{}.key", "ab".repeat(32))), serde_json::json!({"peerToken": token}).to_string()).unwrap();
    Window { inbox: std::os::unix::net::UnixListener::bind(&socket).unwrap(), token }
}

fn received(window: &Window) -> Vec<serde_json::Value> {
    let (stream, _) = window.inbox.accept().unwrap();
    std::io::BufRead::lines(std::io::BufReader::new(stream)).map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect()
}

fn fleet_proxy(home: &Path, conversation: &str) {
    let session = home.join(".local/state/north/fleet/proxy");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "proxy", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 5, "host_pid": null, "conversation": conversation, "host_dir": session, "role": "proxy", "stopped": false});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
}

fn proxies(fleet: &Fleet) -> Vec<String> {
    fleet.sessions().unwrap().into_iter().filter(|record| record.role() == crate::fleet::model::Role::Proxy && fleet.status(record) != crate::fleet::model::Status::Exited).filter_map(|record| record.conversation).collect()
}

#[test]
fn a_second_session_cannot_hold_the_proxy_name_while_the_first_is_live() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "fleet-proxy");
    let first = open_window(home.path(), std::process::id(), "window-a", 1);
    let _second = open_window(home.path(), std::os::unix::process::parent_id(), "window-b", 2);

    fleet.adopt("proxy", "window-a", "/tmp".into()).unwrap();
    assert_eq!(proxies(&fleet), ["window-a"], "adopt ends the fleet proxy and binds the name to the window");
    assert!(fleet.past_sessions().iter().any(|record| record.conversation.as_deref() == Some("fleet-proxy")));
    assert_eq!(fleet.session("proxy").unwrap().addressed_since(), 5, "the window takes every message sent since the old holder began");
    let refused = fleet.revive("proxy", None).unwrap_err().user_message();
    assert!(refused.contains("interactive window"), "{refused}");
    let followed = fleet.follow_recent("proxy").expect("north fleet lists an adopted name it cannot tail");
    assert_eq!(followed.transcript().status, crate::fleet::model::Status::Idle, "the fleet window shows the live window idle");

    let sender = std::thread::spawn(move || fleet.send_message(&crate::fleet::sender::Sender::Session("lead".into()), "proxy", "hello", crate::fleet::model::Delivery::Queued).map(|_| fleet));
    let lines = received(&first);
    let fleet = sender.join().unwrap().unwrap();
    assert_eq!(lines[0], serde_json::json!({"type": "auth", "token": first.token}));
    assert_eq!(lines[1]["type"], "user");
    assert!(lines[1]["message"]["content"].as_str().unwrap().ends_with("hello"), "{}", lines[1]);
    assert!(fleet.session("proxy").unwrap().adopted(), "the message started no host under the name");

    fleet.adopt("proxy", "window-b", "/tmp".into()).unwrap();
    assert_eq!(proxies(&fleet), ["window-b"], "a second window takes the name only by ending the first holder");
}

#[test]
fn the_session_start_adoption_takes_a_free_name_and_leaves_a_name_a_live_session_holds() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "fleet-proxy");
    let _first = open_window(home.path(), std::process::id(), "window-a", 1);
    let _second = open_window(home.path(), std::os::unix::process::parent_id(), "window-b", 2);

    assert!(fleet.adopt_if_free("proxy", "not-a-window", "/tmp".into()).unwrap().is_none(), "a conversation that is no interactive window adopts nothing");
    assert!(fleet.adopt_if_free("proxy", "window-a", "/tmp".into()).unwrap().is_some(), "no live session held the name");
    assert_eq!(proxies(&fleet), ["window-a"]);

    assert!(fleet.adopt_if_free("proxy", "window-b", "/tmp".into()).unwrap().is_none(), "window-a is live and holds the name");
    assert_eq!(proxies(&fleet), ["window-a"]);
    assert!(fleet.adopt_if_free("proxy", "window-a", "/tmp".into()).unwrap().is_some(), "the holder's own start again keeps the name");
}

#[test]
fn a_launch_or_message_naming_the_proxy_whose_holder_died_binds_a_live_window_its_own_conversation_first_and_revives_nothing() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "fleet-proxy");
    let stopped = home.path().join(".local/state/north/fleet/proxy/session.json");
    fs::write(&stopped, fs::read_to_string(&stopped).unwrap().replace("\"stopped\":false", "\"stopped\":true")).unwrap();
    let mut holder = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let _held = open_window(home.path(), holder.id(), "fleet-proxy", 5);
    let _older = open_window(home.path(), std::os::unix::process::parent_id(), "window-older", 6);
    let newest = open_window(home.path(), std::process::id(), "window-newest", 9);
    assert!(proxies(&fleet).is_empty(), "the fleet proxy's host is gone while a window resumed its conversation");
    assert!(fleet.listens("proxy").unwrap(), "a launch naming the proxy as parent finds it live without anyone running adopt");
    assert_eq!(proxies(&fleet), ["fleet-proxy"], "the window on the holder's own conversation takes the name before any newer window");
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(proxies(&fleet).is_empty(), "the holder's window is gone");

    let sender = std::thread::spawn(move || fleet.send_message(&crate::fleet::sender::Sender::Session("lead".into()), "proxy", "main is red", crate::fleet::model::Delivery::Queued).map(|_| fleet));
    let lines = received(&newest);
    let fleet = sender.join().unwrap().unwrap();

    assert_eq!(lines[0], serde_json::json!({"type": "auth", "token": newest.token}));
    assert!(lines[1]["message"]["content"].as_str().unwrap().ends_with("main is red"), "{}", lines[1]);
    assert_eq!(proxies(&fleet), ["window-newest"]);
    assert!(fleet.session("proxy").unwrap().adopted(), "no host was started under the name");
}

#[test]
fn an_adopted_name_leaves_no_socket_a_session_host_of_an_older_runtime_could_bind_and_keeps_the_adoption_through_a_second_adopt() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "fleet-proxy");
    let _window = open_window(home.path(), std::process::id(), "window-a", 1);

    fleet.adopt("proxy", "window-a", "/tmp".into()).unwrap();
    let socket = fleet.root().join("proxy/host.sock");

    assert!(socket.is_dir());
    assert!(fs::remove_file(&socket).is_err(), "an older session host removes a stale socket before it binds, and cannot remove this");
    assert!(std::os::unix::net::UnixListener::bind(&socket).is_err(), "so its bind fails before it starts the provider");
    fleet.adopt("proxy", "window-a", "/tmp".into()).unwrap();
    assert!(socket.is_dir());
    assert!(fleet.session("proxy").unwrap().adopted());
}

#[test]
fn a_revive_or_take_over_on_a_conversation_an_interactive_window_holds_starts_no_host_and_keeps_the_record() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "window-a");
    let _window = open_window(home.path(), std::process::id(), "window-a", 1);

    let revive = fleet.revive("proxy", None).unwrap_err().user_message();
    assert!(revive.contains("interactive window"), "{revive}");
    let request = crate::fleet::model::LaunchRequest {
        verification: false,
        name: "proxy".into(),
        provider: crate::fleet::model::ProviderName::new("claude"),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt: String::new(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: crate::fleet::prompt::PromptMode::default(),
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: Some(crate::fleet::model::Role::Proxy),
        resume: Some("window-a".into()),
        compact_window: None,
        slots: Default::default(),
    };
    let take_over = fleet.take_over(&request).unwrap_err().user_message();

    assert!(take_over.contains("interactive window"), "{take_over}");
    assert_eq!(fleet.session("proxy").unwrap().conversation.as_deref(), Some("window-a"));
    assert!(!fleet.root().join("proxy/host.pid").exists());
}

#[test]
fn an_interactive_window_opened_after_the_turn_leaves_a_running_first_turn_to_finish() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let session = fleet.root().join("proxy");
    let record = serde_json::json!({"name": "proxy", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 5, "host_pid": std::process::id(), "conversation": "c", "host_dir": session, "role": "proxy", "stopped": false});
    fs::create_dir_all(session.join("g1")).unwrap();
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let stream = session.join("g1/stream.jsonl");
    let frame = |kind: &str| serde_json::json!({"type": kind, "subtype": "success", "session_id": "c", "message": {"content": [{"type": "text", "text": "first turn"}]}}).to_string() + "\n";
    fs::write(&stream, frame("assistant")).unwrap();
    let mut turn_over_at_launch = None;
    let mut looks = 0;
    fleet
        .interactive_pacing(
            "proxy",
            true,
            || {
                looks += 1;
                std::io::Write::write_all(&mut fs::OpenOptions::new().append(true).open(&stream).unwrap(), frame("result").as_bytes()).unwrap();
            },
            |_| {
                turn_over_at_launch = Some(fs::read_to_string(&stream).unwrap().contains("\"type\":\"result\""));
                Ok(())
            },
        )
        .unwrap();

    assert_eq!(looks, 1, "the turn was still running at the first look and over at the second");
    assert_eq!(turn_over_at_launch, Some(true), "the window took the conversation while the first turn was still running");
}

#[test]
fn an_interactive_window_that_would_stop_at_a_trust_or_bypass_prompt_is_named_before_it_opens() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_proxy(home.path(), "c");
    let profile = home.path().join(".local/state/north/profiles/claude-a");

    let untrusted = fleet.interactive_prompt("proxy").unwrap().expect("an untrusted folder opens a trust prompt");
    assert!(untrusted.contains("trust"), "{untrusted}");
    fs::write(profile.join(".claude.json"), r#"{"projects":{"/tmp":{"hasTrustDialogAccepted":true}}}"#).unwrap();
    let bypass = fleet.interactive_prompt("proxy").unwrap().expect("bypass mode asks for confirmation until the profile accepts it");
    assert!(bypass.contains("bypass"), "{bypass}");
    fs::write(profile.join("settings.json"), r#"{"skipDangerousModePermissionPrompt":true}"#).unwrap();

    assert_eq!(fleet.interactive_prompt("proxy").unwrap(), None);
}

#[test]
fn a_session_relaunched_or_revived_mid_turn_is_handed_a_turn_that_carries_on_and_an_idle_one_is_not() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let working = crate::fleet::registry::fixtures::live(&fleet, "working");
    let idle = crate::fleet::registry::fixtures::live(&fleet, "idle");
    let prompt = serde_json::json!({"type": "user", "session_id": "working", "uuid": "p1", "message": {"content": "split the crate"}});
    let tool = serde_json::json!({"type": "assistant", "session_id": "working", "message": {"content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "cargo check"}}]}});
    fs::write(fleet.root().join("working/g1/stream.jsonl"), format!("{prompt}\n{tool}\n")).unwrap();
    let restart = |name: &str| {
        let stopped = fleet.record(name).unwrap();
        fleet.phases().unwrap().live(stopped.launch(), Some(std::process::id()), "restarted").unwrap();
        fleet.record(name)
    };

    fleet.relaunch_with("working", Some("opus"), None, restart).unwrap();
    fleet.relaunch_with("idle", Some("opus"), None, restart).unwrap();

    let frames = working.frames.lock().unwrap();
    assert!(frames.iter().any(|frame| frame.contains("relaunched on opus in the middle of a turn")), "{frames:?}");
    assert!(idle.frames.lock().unwrap().iter().all(|frame| !frame.contains("relaunched")), "an idle session was handed a turn");

    let parked = fleet.root().join("parked");
    fs::create_dir_all(parked.join("g1")).unwrap();
    let record = serde_json::json!({"name": "parked", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "working", "host_dir": parked, "stream_offset": 0});
    fs::write(parked.join("session.json"), record.to_string()).unwrap();
    fs::write(parked.join("g1/stream.jsonl"), format!("{prompt}\n{tool}\n")).unwrap();
    assert!(fleet.left_mid_turn("parked").unwrap(), "a revive of a host that exited mid-turn would hand it no turn");
    assert!(!fleet.left_mid_turn("idle").unwrap());
}
