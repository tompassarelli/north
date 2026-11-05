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

