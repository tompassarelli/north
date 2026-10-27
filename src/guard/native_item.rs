mod corpus;
mod gh;

use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub(super) use gh::writes_to_github;

const REASON: &str = "No. This gh command names a native NorthThread item (life/accounts/travel/ops#N) or quotes its text, and native NorthThread items never reach GitHub. Record it with `threads note REF TEXT` instead, and rewrite the GitHub text without the ref or the quoted item text.";
const BODY_FILE_LIMIT: usize = 1 << 20;

static NATIVE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:life|accounts|travel|ops)#\d+\b").unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(command) = input.command().and_then(|s| s.as_str()) else {
        return Verdict::Allow;
    };
    if !writes_to_github(command) || crate::hook::authoring_guards_off("native-item-guard") {
        return Verdict::Allow;
    }
    let cwd = input
        .cwd()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    if names_native_item(command, &cwd, &corpus::databases()) {
        Verdict::Deny(REASON.into())
    } else {
        Verdict::Allow
    }
}

pub(super) fn names_native_item(command: &str, cwd: &Path, databases: &[PathBuf]) -> bool {
    let mut text = command.to_owned();
    for path in gh::body_files(command, cwd) {
        text.push('\n');
        text.push_str(&read_bounded(&path));
    }
    NATIVE_REF.is_match(&text) || corpus::quotes_native_text(&text, databases)
}

fn read_bounded(path: &Path) -> String {
    use std::io::Read;
    let mut bytes = Vec::new();
    let _ = std::fs::File::open(path)
        .and_then(|file| file.take(BODY_FILE_LIMIT as u64).read_to_end(&mut bytes));
    String::from_utf8_lossy(&bytes).into_owned()
}
