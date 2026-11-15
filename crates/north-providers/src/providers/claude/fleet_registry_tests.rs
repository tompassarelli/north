use std::fs;
use std::path::Path;
use std::sync::Arc;

use north_fleet_supervisor::fleet::registry::*;
use north_fleet_supervisor::fleet::claims::{self};
use north_fleet_supervisor::fleet::model::EntryKind;
use north_fleet_supervisor::fleet::runtimes::Runtimes;
use north_fleet_supervisor::fleet::test_support::Internals;
use north_fleet_supervisor::fleet::fakes::Paper;
use super::Claude;

fn fleet(home: &Path, profiles: &str) -> Fleet {
    fleet_claiming(home, profiles, Arc::new(claims::Held::default()))
}

fn fleet_claiming(home: &Path, profiles: &str, held: Arc<claims::Held>) -> Fleet {
    let local = home.join(".local/state/north/profiles/claude-a");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join(".credentials.json"), "{}").unwrap();
    fs::write(local.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"local-login"}}"#).unwrap();
    let policy = north_base::platform::orchestration::under(home);
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    let here = north_base::platform::host::name();
    fs::write(&policy, format!("[accounts.claude.profiles.a]\n\"{here}\" = {{ path = \"~/.local/state/north/profiles/claude-a\" }}\n{profiles}")).unwrap();
    Fleet::at_claiming(home.into(), Runtimes::new(vec![Arc::new(Claude)]), Arc::new(Paper::default()), held).unwrap()
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
    open_window_on(home, "a", pid, conversation, started)
}

fn open_window_on(home: &Path, account: &str, pid: u32, conversation: &str, started: u64) -> Window {
    let sessions = home.join(format!(".local/state/north/profiles/claude-{account}/sessions"));
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

fn fleet_coordinator(home: &Path, conversation: &str) {
    let session = home.join(".local/state/north/fleet/coordinator");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "coordinator", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 5, "host_pid": null, "conversation": conversation, "host_dir": session, "role": "coordinator", "stopped": false});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
}

fn proxies(fleet: &Fleet) -> Vec<String> {
    fleet.sessions().unwrap().into_iter().filter(|record| record.role() == north_fleet_supervisor::fleet::model::Role::Coordinator && fleet.status(record) != north_fleet_supervisor::fleet::model::Status::Exited).filter_map(|record| record.conversation).collect()
}

#[test]
fn a_second_session_cannot_hold_the_coordinator_name_while_the_first_is_live() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_coordinator(home.path(), "fleet-coordinator");
    let first = open_window(home.path(), std::process::id(), "window-a", 1);
    let _second = open_window(home.path(), std::os::unix::process::parent_id(), "window-b", 2);

    fleet.adopt("coordinator", "window-a", "/tmp".into()).unwrap();
    assert_eq!(proxies(&fleet), ["window-a"], "adopt ends the fleet coordinator and binds the name to the window");
    assert!(fleet.past_sessions().iter().any(|record| record.conversation.as_deref() == Some("fleet-coordinator")));
    assert_eq!(fleet.session("coordinator").unwrap().addressed_since(), 5, "the window takes every message sent since the old holder began");
    let refused = fleet.revive("coordinator", None).unwrap_err().user_message();
    assert!(refused.contains("interactive window"), "{refused}");
    let followed = fleet.follow_recent("coordinator").expect("north fleet lists an adopted name it cannot tail");
    assert_eq!(followed.transcript().status, north_fleet_supervisor::fleet::model::Status::Idle, "the fleet window shows the live window idle");

    let sender = std::thread::spawn(move || fleet.send_message(&north_fleet_supervisor::fleet::sender::Sender::Session("lead".into()), "coordinator", "hello", north_fleet_supervisor::fleet::model::Delivery::Queued).map(|_| fleet));
    let lines = received(&first);
    let fleet = sender.join().unwrap().unwrap();
    assert_eq!(lines[0], serde_json::json!({"type": "auth", "token": first.token}));
    assert_eq!(lines[1]["type"], "user");
    assert!(lines[1]["message"]["content"].as_str().unwrap().ends_with("hello"), "{}", lines[1]);
    assert!(fleet.session("coordinator").unwrap().adopted(), "the message started no host under the name");

    fleet.adopt("coordinator", "window-b", "/tmp".into()).unwrap();
    assert_eq!(proxies(&fleet), ["window-b"], "a second window takes the name only by ending the first holder");
}

#[test]
fn a_launch_or_message_naming_the_coordinator_whose_holder_died_binds_a_window_on_its_own_conversation_only_and_reaches_no_other() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_coordinator(home.path(), "fleet-coordinator");
    let stopped = home.path().join(".local/state/north/fleet/coordinator/session.json");
    fs::write(&stopped, fs::read_to_string(&stopped).unwrap().replace("\"stopped\":false", "\"stopped\":true")).unwrap();
    let mut holder = std::process::Command::new("sleep").arg("600").spawn().unwrap();
    let _held = open_window(home.path(), holder.id(), "fleet-coordinator", 5);
    let _older = open_window(home.path(), std::os::unix::process::parent_id(), "window-older", 6);
    let newest = open_window(home.path(), std::process::id(), "window-newest", 9);
    assert!(proxies(&fleet).is_empty(), "the fleet coordinator's host is gone while a window resumed its conversation");
    assert!(fleet.listens("coordinator").unwrap(), "a launch naming the coordinator as parent finds it live without anyone running adopt");
    assert_eq!(proxies(&fleet), ["fleet-coordinator"], "the window on the holder's own conversation takes the name before any newer window");
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(proxies(&fleet).is_empty(), "the holder's window is gone");

    newest.inbox.set_nonblocking(true).unwrap();
    let Err(refusal) = fleet.send_message(&north_fleet_supervisor::fleet::sender::Sender::Session("lead".into()), "coordinator", "main is red", north_fleet_supervisor::fleet::model::Delivery::Queued) else {
        panic!("a message to the dead name was accepted");
    };
    assert!(refusal.user_message().contains("north fleet adopt --name coordinator"), "{}", refusal.user_message());
    assert!(proxies(&fleet).is_empty(), "no window on the holder's own conversation is left, so the name stays unbound");
    assert_eq!(newest.inbox.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock, "window-newest, on another conversation, receives nothing");
}

#[test]
fn a_revive_or_take_over_on_a_conversation_an_interactive_window_holds_starts_no_host_and_keeps_the_record() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    fleet_coordinator(home.path(), "window-a");
    let _window = open_window(home.path(), std::process::id(), "window-a", 1);

    let revive = fleet.revive("coordinator", None).unwrap_err().user_message();
    assert!(revive.contains("interactive window"), "{revive}");
    let request = north_fleet_supervisor::fleet::model::LaunchRequest {
        verification: false,
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: "coordinator".into(),
        provider: north_fleet_supervisor::fleet::model::ProviderName::new("claude"),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt: String::new(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: north_fleet_supervisor::fleet::prompt::PromptMode::default(),
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: Some(north_fleet_supervisor::fleet::model::Role::Coordinator),
        resume: Some("window-a".into()),
        compact_window: None,
        dispatched: false,
        surface: Default::default(),
        route: None,
            };
    let take_over = fleet.take_over(&request).unwrap_err().user_message();

    assert!(take_over.contains("interactive window"), "{take_over}");
    assert_eq!(fleet.session("coordinator").unwrap().conversation.as_deref(), Some("window-a"));
    assert!(!fleet.root().join("coordinator/host.pid").exists());
}

#[test]
fn a_forgotten_session_with_no_retired_note_still_shows_its_final_report_from_its_archive_north_706() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    let session = fleet.root().join("wisp-230-d2020");
    let record = serde_json::json!({"name": "wisp-230-d2020", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "c", "host_dir": session, "stream_offset": 0});
    fs::create_dir_all(session.join("g1")).unwrap();
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let assistant = serde_json::json!({"type": "assistant", "session_id": "c", "message": {"content": [{"type": "text", "text": "Not done: the box needs the laptop.\nStop: host"}]}}).to_string();
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": "c"}).to_string();
    fs::write(session.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
    fleet.forget("wisp-230-d2020").unwrap();

    assert!(fleet.follow("wisp-230-d2020").is_err(), "the registry no longer lists it");
    let follower = fleet.follow_retired("wisp-230-d2020").unwrap();

    let replies: Vec<_> = follower.transcript().entries.iter().filter(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.as_str()).collect();
    assert_eq!(replies, ["Not done: the box needs the laptop.\nStop: host"], "dispatch noted its report unread and relaunched the box");
}
