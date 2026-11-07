use crate::hook::{HookInput, Verdict};
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const SEP: &str = r"(?:^|[\n;&|({`]|\$\()";
const WRAP: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|timeout\s+\S+|north\s+capacity\s+bounded\s+\S+\s+--)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const PATHED: &str = r"(?:\S*/)?";
const HINTS: &[&str] = &["wl-copy", "wl-paste", "xclip", "xsel", "screenshot"];
const REASON: &str = "No. Never read, write or clear Tom's clipboard; capture screens only with north desktop capture, never niri screenshot actions. Capture with `north desktop capture --screen OUT.png` (or --region \"X,Y WxH\" OUT.png, or --window ID OUT.png for a visible floating window) and pass the file path.";

static CLIPBOARD_ACCESS: LazyLock<Regex> = LazyLock::new(|| {
    let head = format!(r"{SEP}\s*{WRAP}{PATHED}");
    Regex::new(&format!(
        r"{head}(?:wl-copy\b|(?:wl-paste|xclip|xsel)\b|niri\s+msg\b[^\n;&|)}}`]*?\baction\s+screenshot(?:-screen|-window)?\b)"
    ))
    .unwrap()
});
static LOOKUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bcommand\s+-[vV]\s+\S+").unwrap());
static SCREENSHOT_ACCESS: LazyLock<Regex> = LazyLock::new(|| {
    let head = format!(r"{SEP}\s*{WRAP}{PATHED}");
    Regex::new(&format!(r"{head}niri\s+msg\b[^\n;&|)}}`]*?\baction\s+screenshot(?:-screen|-window)?\b")).unwrap()
});
static PRIVATE_RUNTIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s;&|(])(?:export\s+)?(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*XDG_RUNTIME_DIR=[\x22']?/run/user/[0-9]+/private-desktop\.[A-Za-z0-9]+/runtime[\x22']?(?:\s|;|$)").unwrap()
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
    if !HINTS.iter().any(|hint| command.contains(hint)) {
        return false;
    }
    let scan = shell::strip_quotes(&LOOKUP.replace_all(&shell::strip_heredocs(command), ""));
    let access = if PRIVATE_RUNTIME.is_match(command) { &SCREENSHOT_ACCESS } else { &CLIPBOARD_ACCESS };
    access.is_match(&scan)
}
