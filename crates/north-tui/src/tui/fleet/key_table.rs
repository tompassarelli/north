use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Quit,
    NewSession,
    Interrupt,
    Stop,
    Forget,
    Deepen,
    Digest,
    PasteImage,
    CopyReply,
}

pub struct Binding {
    pub keys: &'static str,
    pub description: &'static str,
    pub when: &'static str,
    pub control: Option<(char, Control)>,
}

const fn key(keys: &'static str, description: &'static str, when: &'static str) -> Binding {
    Binding { keys, description, when, control: None }
}

const fn ctrl(keys: &'static str, character: char, control: Control, description: &'static str) -> Binding {
    Binding { keys, description, when: "anywhere", control: Some((character, control)) }
}

pub const BINDINGS: &[Binding] = &[
    key("?", "open this help", "empty composer"),
    key("/", "open the slash commands", "empty composer"),
    key("Enter", "send to the selected session", "composer"),
    key("Alt+Enter, Shift+Enter", "insert a newline", "composer"),
    key("Ctrl+Enter, Alt+U", "send urgently", "composer"),
    ctrl("Ctrl+G", 'g', Control::Deepen, "deepen the message and send it"),
    key("Esc", "interrupt a working session, else focus the list", "composer"),
    key("Ctrl+A, Ctrl+E", "start, end of line", "composer"),
    key("Ctrl+K, Ctrl+U", "delete to line end, line start", "composer"),
    key("Ctrl+W", "delete the word before the cursor", "composer"),
    key("Ctrl+B, Ctrl+F", "move the cursor back, forward", "composer"),
    key("j/k, ↑/↓", "select a session", "list"),
    key("g/Home, G/End", "first, last session", "list"),
    key("Enter", "focus the composer", "list"),
    key("Esc", "jump to the latest output", "list"),
    key("r", "resume the selected exited session", "list"),
    key("any letter", "start a message", "list"),
    key("Tab, Shift+Tab", "switch between list and composer", "anywhere"),
    key("PgUp/PgDn", "scroll the transcript", "anywhere"),
    ctrl("Ctrl+N", 'n', Control::NewSession, "start a new session"),
    ctrl("Ctrl+C", 'c', Control::Interrupt, "interrupt the selected session"),
    ctrl("Ctrl+X", 'x', Control::Stop, "stop the selected session"),
    ctrl("Ctrl+D", 'd', Control::Forget, "forget the selected exited session"),
    ctrl("Ctrl+T", 't', Control::Digest, "toggle the gate digest"),
    ctrl("Ctrl+Y", 'y', Control::CopyReply, "copy the last reply"),
    ctrl("Ctrl+V", 'v', Control::PasteImage, "paste an image from the clipboard"),
    ctrl("Ctrl+Q", 'q', Control::Quit, "quit"),
    key("j/k, ↑/↓", "select a gate", "digest, list"),
    key("1-9", "answer with that option", "digest, list"),
    key("type, Enter", "answer in your words", "digest"),
    key("Tab, Shift+Tab", "next, previous field", "new session"),
    key("←/→", "choose a provider or account", "new session"),
    key("Enter", "launch", "new session"),
    key("Alt+Enter", "newline in the prompt", "new session"),
    key("Esc", "cancel", "new session"),
    key("y, Enter", "stop the session", "stop prompt"),
    key("n, Esc", "keep it running", "stop prompt"),
    key("←/→, Tab, Ctrl+H, Ctrl+L", "switch view", "this help"),
    key("↑/↓", "move the cursor", "this help"),
    key("Esc", "close", "this help"),
];

pub fn control_for(event: KeyEvent) -> Option<Control> {
    let KeyCode::Char(character) = event.code else { return None };
    if !event.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    let character = character.to_ascii_lowercase();
    BINDINGS.iter().find_map(|binding| binding.control.filter(|(bound, _)| *bound == character).map(|(_, control)| control))
}
