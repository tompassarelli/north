use std::path::Path;

use crossterm::event::{Event, KeyCode};

use super::*;
use crate::tui::fleet::attach::Attachments;
use crate::tui::fleet::clipboard::{ClipImage, Clipboard};
use crate::tui::fleet::screen::Notice;

struct FakeClipboard(Option<ClipImage>);

impl Clipboard for FakeClipboard {
    fn image(&self) -> NorthResult<Option<ClipImage>> {
        Ok(self.0.clone())
    }
}

fn screen_with_clipboard(desk: &DeskFake, image: Option<Vec<u8>>, state: &Path) -> Screen {
    let image = image.map(|bytes| ClipImage { extension: "png", bytes });
    screen(desk).with_attachments(Attachments::new(Box::new(FakeClipboard(image)), state.join(".attachments")))
}

#[test]
fn ctrl_v_attaches_the_clipboard_image_as_a_chip_and_sends_its_stored_path() {
    let state = tempfile::tempdir().unwrap();
    let mut desk = desk();
    let mut screen = screen_with_clipboard(&desk, Some(vec![7; 34 * 1024]), state.path());
    type_text(&mut desk, &mut screen, "what is this? ");
    key(&mut desk, &mut screen, control('v'));
    assert_eq!(screen.composer.lines().join("\n"), "what is this? [image 1: 34 KB png]");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let performed = desk.performed();
    let [Command::Send { text, .. }] = performed.as_slice() else { panic!("expected one send, got {performed:?}") };
    let stored = std::fs::read_dir(state.path().join(".attachments")).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(&stored).unwrap(), vec![7; 34 * 1024]);
    assert_eq!(text, &format!("what is this? [image: {}]", stored.display()));
    assert_eq!(screen.pending_for("parser"), [text.as_str()]);
}

#[test]
fn an_empty_bracketed_paste_attaches_the_image_and_a_deleted_chip_sends_no_image() {
    let state = tempfile::tempdir().unwrap();
    let mut desk = desk();
    let mut screen = screen_with_clipboard(&desk, Some(vec![1, 2, 3]), state.path());
    assert!(apply_event(&mut screen, Event::Paste(String::new())).is_none());
    assert_eq!(screen.composer.lines().join("\n"), "[image 1: 1 KB png]");
    screen.composer = tui_textarea::TextArea::default();
    type_text(&mut desk, &mut screen, "only words");
    key(&mut desk, &mut screen, press(KeyCode::Enter));
    let performed = desk.performed();
    let [Command::Send { text, .. }] = performed.as_slice() else { panic!("expected one send, got {performed:?}") };
    assert_eq!(text, "only words");
}

#[test]
fn ctrl_v_without_a_clipboard_image_says_so_and_leaves_the_draft() {
    let state = tempfile::tempdir().unwrap();
    let mut desk = desk();
    let mut screen = screen_with_clipboard(&desk, None, state.path());
    type_text(&mut desk, &mut screen, "draft");
    key(&mut desk, &mut screen, control('v'));
    assert_eq!(screen.composer.lines().join("\n"), "draft");
    assert!(matches!(&screen.notice, Some(Notice::Error(text)) if text.contains("no image on the clipboard")));
}
