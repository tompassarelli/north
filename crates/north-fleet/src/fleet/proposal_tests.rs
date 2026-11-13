use north_base::platform::brief_trailers;

use super::*;

const CHECK: &str = "cargo nextest run -p north-fleet -E 'test(/a_finish_message/)'";
const BOX: &str = "A lane handed in by a session lands with both trailers";

fn brief(done_when: &str) -> String {
    format!("Item: north#1367 Landed commits carry no Box trailer\nBox: {BOX}\nCategory: bug-known-cause\nDone when: {done_when}\n")
}

#[test]
fn a_finish_message_without_trailers_takes_item_and_box_from_the_brief() {
    let brief = brief(&format!("`{CHECK}` passes, and your report pastes it"));
    let proposed = message(&brief).unwrap();
    assert!(proposed.ends_with("\n\nItem: north#1367"), "the proposal carries the item alone: {proposed}");
    let kept = trailers(&brief).unwrap();
    assert_eq!(kept, format!("Item: north#1367\nBox: {BOX}\nCheck: {CHECK}"));
    let both = format!("Add the note (north#1367)\n\nItem: north#1367\nBox: {BOX}");
    assert_eq!(brief_trailers::apply("Add the note (north#1367)", &kept, Some(CHECK)), both, "a finish whose check is the brief's Done when completes its box");
    assert_eq!(brief_trailers::apply(&proposed, &kept, Some(&format!(" {CHECK}"))), format!("{proposed}\nBox: {BOX}"));
    let item = "Add the note (north#1367)\n\nItem: north#1367";
    assert_eq!(brief_trailers::apply("Add the note (north#1367)", &kept, Some("cargo test")), item, "a finish with another check is one step of the box");
    assert_eq!(brief_trailers::apply("Add the note (north#1367)", &kept, None), item, "a hand-in is one step of the box");
    assert_eq!(brief_trailers::apply(&both, &kept, Some(CHECK)), both);
    let typed = "Add the note\n\nItem: north#1367\nBox: its own";
    assert_eq!(brief_trailers::apply(typed, &kept, Some(CHECK)), typed);
    let other = "Add the note\n\nItem: north#12";
    assert_eq!(brief_trailers::apply(other, &kept, Some(CHECK)), other, "another item's message takes nothing");
    let bare = trailers("Item: north#1367 Landed commits carry no Box trailer\nBox: a box\n").unwrap();
    assert_eq!(bare, "Item: north#1367", "a brief with no Done-when command keeps its item alone");
    assert_eq!(brief_trailers::apply("Add the note (north#1367)", &bare, Some(CHECK)), item);
}
