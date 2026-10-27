use crate::hook::{HookInput, Verdict};
use crate::shell;

const PREFIXES: &[&str] = &[
    "env",
    "exec",
    "command",
    "nohup",
    "setsid",
    "nice",
    "ionice",
    "sudo",
    "doas",
    "stdbuf",
    "timeout",
    "dbus-run-session",
    "time",
    "gamemoderun",
];
const LAUNCHERS: &[&str] = &["wine", "wine64", "proton", "steam-run", "umu-run"];
const HINTS: &[&str] = &["wine", "proton", "steam-run", "umu-run", ".exe"];
const REASON: &str = "No. A Wine/Proton/Warcraft client started here runs in your terminal scope, outside capacity admission, and can saturate the GPU. Wrap it: bun \"$(dirname \"$(agents path machine-capacity)\")/scripts/machine-capacity.mjs\" session --class native --new-clients 1 --owner OWNER -- COMMAND ARG... (one session per client; retry DEFER after 30 s). Self-wrapping routes: `wisp lan pool`, ~/.local/share/wisp/online/launch.sh.";

pub fn evaluate(input: &HookInput) -> Verdict {
    match input.command().and_then(|s| s.as_str()) {
        Some(command)
            if launches_unwrapped_client(command)
                && !crate::hook::authoring_guards_off("native-launch-guard") =>
        {
            Verdict::Deny(REASON.into())
        }
        _ => Verdict::Allow,
    }
}

pub(super) fn launches_unwrapped_client(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    if !HINTS.iter().any(|hint| lowered.contains(hint)) {
        return false;
    }
    let Some(words) = shell::words(command) else {
        return false;
    };
    !in_native_session(&words)
        && shell::segments(&words)
            .iter()
            .any(|segment| launches(segment))
}

fn in_native_session(words: &[String]) -> bool {
    words.iter().enumerate().any(|(index, word)| {
        let name = word.strip_suffix(".mjs").unwrap_or(word);
        let rest = &words[index + 1..];
        name.ends_with("machine-capacity")
            && rest.iter().any(|w| w == "session")
            && (rest.iter().any(|w| w == "--class=native")
                || rest.windows(2).any(|pair| pair[0] == "--class" && pair[1] == "native"))
    })
}

fn launches(words: &[String]) -> bool {
    let index = shell::command(words, PREFIXES);
    let Some(first) = words.get(index) else {
        return false;
    };
    let wraps_helper = words[index..]
        .iter()
        .take(2)
        .any(|word| word.ends_with("machine-capacity.mjs"));
    if wraps_helper && let Some(split) = words[index..].iter().position(|word| word == "--") {
        return launches(&words[index + split + 1..]);
    }
    let name = shell::basename(first);
    LAUNCHERS.contains(&name) || name.to_lowercase().ends_with(".exe")
}
