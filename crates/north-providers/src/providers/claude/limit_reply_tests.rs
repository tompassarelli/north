use serde_json::json;

use super::Claude;
use crate::fleet::model::{EntryKind, Transcript};
use crate::fleet::runtime::Runtime;
use crate::usage::decide::world::ModelFamily;
use crate::usage::{events, meter};

#[test]
fn a_model_limit_reply_ends_its_turn_as_a_failed_usage_limit_and_marks_the_account_full_for_that_model_until_its_reset() {
    let reply = "You've hit your Fable limit · resets Nov 19, 9am";
    let assistant = json!({"type": "assistant", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": reply}]}});
    let mut transcript = Transcript::default();
    for frame in [json!({"type": "user", "uuid": "u1", "message": {"content": "land it"}}), assistant.clone(), json!({"type": "result", "subtype": "success", "is_error": false, "result": reply})] {
        Claude.observe(&frame, &mut transcript);
    }
    let kinds: Vec<_> = transcript.entries.iter().map(|entry| (entry.kind, entry.text.as_str())).collect();
    assert_eq!(kinds[kinds.len() - 2..], [(EntryKind::Error, reply), (EntryKind::TurnEnd, "usage-limit")], "{kinds:?}");

    let at = 1_794_000_000.0;
    let rows = events::stream_rows(&assistant, at);
    let window = rows.iter().find(|row| row["kind"] == "window").unwrap();
    assert_eq!((window["name"].as_str(), window["used_pct"].as_f64(), window["resets_at"].as_str()), (Some(meter::scoped(ModelFamily::Fable).as_str()), Some(100.0), Some("2026-11-19T09:00:00Z")), "{rows:?}");
}
