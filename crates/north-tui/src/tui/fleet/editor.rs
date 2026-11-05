use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::TextArea;

pub fn edit_textarea(textarea: &mut TextArea<'static>, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'u'))
    {
        textarea.delete_line_by_head();
    } else {
        textarea.input(key);
    }
}
