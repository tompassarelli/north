use crate::guard::proxy_role;
use crate::hook::{HookInput, Verdict};
use crate::platform::process;
use std::process::Command;
use std::time::Duration;

const DIGEST_LIMIT: Duration = Duration::from_secs(5);

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(proxy_role::is_tom_window(&proxy_role::Live, input), read_digest)
}

pub fn decide(tom_window: bool, digest: impl FnOnce() -> Option<String>) -> Verdict {
    if !tom_window {
        return Verdict::Allow;
    }
    match digest().as_deref().map(str::trim_end) {
        None | Some("") => Verdict::Allow,
        Some(text) => Verdict::Advise(format!("Tom's decision queue (`north gate digest`); show it to him first, as written:\n{text}")),
    }
}

fn read_digest() -> Option<String> {
    let mut command = Command::new(std::env::var("NORTH_BIN").unwrap_or_else(|_| "north".to_owned()));
    command.args(["gate", "digest"]);
    let output = process::run_command_within(command, None, DIGEST_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
