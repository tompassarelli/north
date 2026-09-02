use std::path::PathBuf;

use arboard::{Clipboard, ImageData};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::{ColorType, ImageFormat};
use ratatui::style::{Color, Modifier, Style};
use tempfile::{Builder, NamedTempFile};
use tui_textarea::{AtomicRange, TextArea, WrapMode};

use crate::clause_state::AttachmentIdentity;

pub(crate) struct Composer {
    textarea: TextArea<'static>,
    images: Vec<ImageAttachment>,
}

pub(crate) struct Submission {
    pub(crate) text: String,
    images: Vec<ImageAttachment>,
}

impl Submission {
    pub(crate) fn image_paths(
        &self,
        identities: &[AttachmentIdentity],
    ) -> Result<Vec<PathBuf>, String> {
        if self.images.len() != identities.len() {
            return Err("Clause and the image handle store disagree on attachment count".into());
        }
        identities
            .iter()
            .map(|identity| {
                self.images
                    .iter()
                    .find(|image| image.identity == *identity)
                    .map(|image| image.file.path().to_owned())
                    .ok_or_else(|| {
                        format!(
                            "Clause submitted image {} without a retained file handle",
                            identity.number()
                        )
                    })
            })
            .collect()
    }

    pub(crate) fn attachment_identities(&self) -> Vec<AttachmentIdentity> {
        self.images.iter().map(|image| image.identity).collect()
    }
}

struct ImageAttachment {
    identity: AttachmentIdentity,
    placeholder: String,
    file: NamedTempFile,
}

impl Composer {
    pub(crate) fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        textarea.set_max_rows(8);
        textarea.set_style(
            Style::default()
                .fg(Color::Rgb(229, 231, 235))
                .bg(Color::Rgb(37, 39, 45)),
        );
        textarea.set_cursor_line_style(Style::default());
        textarea.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        Self {
            textarea,
            images: Vec::new(),
        }
    }

    pub(crate) fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    pub(crate) fn measure(&mut self, width: u16) -> u16 {
        self.textarea.measure(width).preferred_rows
    }

    pub(crate) fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.textarea.is_empty() && self.images.is_empty()
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        self.textarea.insert_str(text);
        self.sync_image_placeholders();
    }

    pub(crate) fn replace_text(&mut self, text: &str) -> Vec<AttachmentIdentity> {
        let previous = std::mem::replace(self, Self::new());
        let removed = previous
            .images
            .into_iter()
            .map(|image| image.identity)
            .collect();
        self.insert_text(text);
        removed
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Vec<AttachmentIdentity> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'u'))
        {
            self.textarea.delete_line_by_head();
        } else {
            self.textarea.input(key);
        }
        self.sync_image_placeholders()
    }

    pub(crate) fn read_clipboard_image() -> Result<NamedTempFile, String> {
        let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
        let image = clipboard.get_image().map_err(|error| error.to_string())?;
        clipboard_image_to_png(&image)
    }

    pub(crate) fn attach_image(&mut self, identity: AttachmentIdentity, file: NamedTempFile) {
        let placeholder = format!("[Image #{}]", identity.number());
        if !self.textarea.is_empty()
            && !self
                .textarea
                .lines()
                .last()
                .and_then(|line| line.chars().last())
                .is_some_and(char::is_whitespace)
        {
            self.textarea.insert_char(' ');
        }
        self.textarea.insert_str(&placeholder);
        self.textarea.insert_char(' ');
        self.images.push(ImageAttachment {
            identity,
            placeholder,
            file,
        });
        self.sync_image_placeholders();
    }

    pub(crate) fn take_submission(&mut self) -> Submission {
        let previous = std::mem::replace(self, Self::new());
        Submission {
            text: previous.text(),
            images: previous.images,
        }
    }

    fn sync_image_placeholders(&mut self) -> Vec<AttachmentIdentity> {
        let mut removed = Vec::new();
        self.images.retain(|image| {
            let retained = self
                .textarea
                .lines()
                .iter()
                .any(|line| line.contains(&image.placeholder));
            if !retained {
                removed.push(image.identity);
            }
            retained
        });
        let mut ranges = Vec::new();
        let mut highlights = Vec::new();
        for (row, line) in self.textarea.lines().iter().enumerate() {
            for image in &self.images {
                let Some(start_byte) = line.find(&image.placeholder) else {
                    continue;
                };
                let end_byte = start_byte + image.placeholder.len();
                let start_col = line[..start_byte].chars().count();
                let end_col = line[..end_byte].chars().count();
                ranges.push(AtomicRange {
                    row,
                    start_col,
                    end_col,
                });
                highlights.push((row, start_byte, end_byte));
            }
        }
        self.textarea.set_atomic_ranges(ranges);
        self.textarea.clear_custom_highlight();
        for (row, start, end) in highlights {
            self.textarea.custom_highlight(
                ((row, start), (row, end)),
                Style::default().fg(Color::Green),
                10,
            );
        }
        removed
    }
}

fn clipboard_image_to_png(image: &ImageData<'_>) -> Result<NamedTempFile, String> {
    let width = u32::try_from(image.width).map_err(|error| error.to_string())?;
    let height = u32::try_from(image.height).map_err(|error| error.to_string())?;
    let file = Builder::new()
        .prefix("north-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|error| error.to_string())?;
    image::save_buffer_with_format(
        file.path(),
        image.bytes.as_ref(),
        width,
        height,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    .map_err(|error| error.to_string())?;
    Ok(file)
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    use super::*;
    use crate::clause_state::NorthState;

    #[test]
    fn editor_supplies_standard_navigation_and_kill_bindings() {
        let mut composer = Composer::new();
        composer.insert_text("alpha beta");
        composer.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        composer.insert_text("start ");
        composer.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        composer.insert_text(" end");

        assert_eq!(composer.text(), "start alpha beta end");

        composer.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(composer.text(), "");
    }

    #[test]
    fn editor_wrap_measurement_grows_for_long_input() {
        let mut composer = Composer::new();
        composer.insert_text("can you simulate making a task list, like a three step plan");

        assert!(composer.measure(24) >= 3);
    }

    #[test]
    fn editor_does_not_underline_the_current_line() {
        let mut composer = Composer::new();
        composer.insert_text("plain text");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));

        composer.textarea().render(buffer.area, &mut buffer);

        assert!(!buffer[(0, 0)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn image_attachment_is_an_atomic_visible_placeholder_and_native_submission() {
        let file = Builder::new()
            .prefix("north-composer-test-")
            .suffix(".png")
            .tempfile()
            .unwrap();
        let expected_path = file.path().to_owned();
        let mut composer = Composer::new();
        composer.insert_text("inspect");
        composer.attach_image(AttachmentIdentity(1), file);

        assert_eq!(composer.text(), "inspect [Image #1] ");
        assert_eq!(composer.textarea.atomic_ranges().len(), 1);

        let submission = composer.take_submission();
        assert_eq!(
            submission.image_paths(&[AttachmentIdentity(1)]).unwrap(),
            vec![expected_path.clone()]
        );
        assert!(expected_path.exists());
        drop(submission);
        assert!(!expected_path.exists());
    }

    #[test]
    fn clipboard_rgba_bytes_are_encoded_as_a_valid_png() {
        let file = clipboard_image_to_png(&ImageData {
            width: 1,
            height: 1,
            bytes: Cow::Owned(vec![0x12, 0x34, 0x56, 0xff]),
        })
        .unwrap();
        let image = image::open(file.path()).unwrap().to_rgba8();

        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.get_pixel(0, 0).0, [0x12, 0x34, 0x56, 0xff]);
    }

    #[test]
    fn clause_selects_the_exact_retained_image_handles_for_submission() {
        let mut state = NorthState::open().expect("North Clause source opens");
        let identity = state
            .attach_image()
            .expect("Clause allocates an image identity");
        let file = Builder::new()
            .prefix("north-submission-test-")
            .suffix(".png")
            .tempfile()
            .unwrap();
        let expected_path = file.path().to_owned();
        let mut composer = Composer::new();
        composer.attach_image(identity, file);

        let submission = composer.take_submission();
        let submitted = state.submit().expect("Clause rolls the draft forward");
        assert_eq!(submission.image_paths(&submitted).unwrap(), [expected_path]);
        state
            .settle_success()
            .expect("Clause clears the submitted set");
    }

    #[test]
    fn deleting_an_atomic_image_placeholder_removes_it_from_the_clause_draft() {
        let mut state = NorthState::open().expect("North Clause source opens");
        let identity = state
            .attach_image()
            .expect("Clause allocates an image identity");
        let file = Builder::new()
            .prefix("north-removal-test-")
            .suffix(".png")
            .tempfile()
            .unwrap();
        let mut composer = Composer::new();
        composer.attach_image(identity, file);

        let removed = composer.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(removed, [identity]);
        state
            .detach_image(identity)
            .expect("Clause withdraws the image");
        assert!(state.submit().expect("empty draft submits").is_empty());
    }
}
