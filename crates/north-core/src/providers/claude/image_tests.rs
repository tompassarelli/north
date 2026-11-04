use serde_json::{Value, json};

use super::*;
use crate::fleet::attachments;

fn frame(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

#[test]
fn a_message_naming_a_stored_image_encodes_a_base64_image_block_and_a_text_block() {
    let state = tempfile::tempdir().unwrap();
    let dir = state.path().join(attachments::DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("abc.png");
    std::fs::write(&path, b"\x89PNG foo").unwrap();
    let text = format!("what is this? {}", attachments::marker(&path));

    let lines = Claude.message(&Transcript::new(), "u1", &text).unwrap();

    let [line] = lines.as_slice() else { panic!("expected one frame, got {lines:?}") };
    let sent = frame(line);
    assert_eq!(sent["uuid"], "u1");
    assert_eq!(
        sent["message"]["content"],
        json!([
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": attachments::base64(b"\x89PNG foo")}},
            {"type": "text", "text": text},
        ])
    );
    assert_eq!(Claude.sent_prompt(&sent).as_deref(), Some("u1"));
}

#[test]
fn a_plain_message_stays_a_text_string_and_a_missing_image_is_refused() {
    let plain = Claude.message(&Transcript::new(), "u2", "hello").unwrap();
    assert_eq!(frame(&plain[0])["message"]["content"], "hello");

    let state = tempfile::tempdir().unwrap();
    let missing = state.path().join(attachments::DIR).join("gone.png");
    let error = Claude.message(&Transcript::new(), "u3", &attachments::marker(&missing)).unwrap_err();
    assert!(error.user_message().contains("cannot read attachment"), "{}", error.user_message());
}

#[test]
fn a_user_echo_with_image_and_text_blocks_projects_the_text() {
    let echo = json!({"type": "user", "isReplay": true, "uuid": "u1", "message": {"role": "user", "content": [
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}},
        {"type": "text", "text": "what is this? [image: /s/.attachments/abc.png]"},
    ]}});
    let mut transcript = Transcript::new();
    Claude.observe(&echo, &mut transcript);
    let kinds: Vec<_> = transcript.entries.iter().map(|entry| (entry.kind, entry.text.as_str())).collect();
    assert_eq!(kinds, [(EntryKind::User, "what is this? [image: /s/.attachments/abc.png]")]);
}
