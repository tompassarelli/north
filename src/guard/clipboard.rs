use crate::hook::{HookInput, Verdict};
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const SEP: &str = r"(?:^|[\n;&|({`]|\$\()";
const WRAP: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|timeout\s+\S+|run-bounded\s+\S+\s+--)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const PATHED: &str = r"(?:\S*/)?";
const HINTS: &[&str] = &["wl-copy", "wl-paste", "xclip", "xsel", "screenshot"];
const REASON: &str = "No. Never read, write or clear Tom's clipboard; capture screens only with agent-capture, never niri screenshot actions. Capture with `agent-capture --screen OUT.png` (or --region \"X,Y WxH\" OUT.png, or --window ID OUT.png for a visible floating window) and pass the file path.";

static CLIPBOARD_ACCESS: LazyLock<Regex> = LazyLock::new(|| {
    let head = format!(r"{SEP}\s*{WRAP}{PATHED}");
    Regex::new(&format!(
        r"{head}(?:wl-copy\b|(?:wl-paste|xclip|xsel)\b|niri\s+msg\b[^\n;&|)}}`]*?\baction\s+screenshot(?:-screen|-window)?\b)"
    ))
    .unwrap()
});

pub fn evaluate(input: &HookInput) -> Verdict {
    match input.command().and_then(|s| s.as_str()) {
        Some(command)
            if touches_clipboard(command) && !crate::guard::activation::authoring_guards_off("clipboard-guard") =>
        {
            Verdict::Deny(REASON.into())
        }
        _ => Verdict::Allow,
    }
}

pub(super) fn touches_clipboard(command: &str) -> bool {
    HINTS.iter().any(|hint| command.contains(hint))
        && CLIPBOARD_ACCESS.is_match(&shell::strip_quotes(&shell::strip_heredocs(command)))
}
