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
    let policy = crate::pace::target::home_policy_path(home);
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

