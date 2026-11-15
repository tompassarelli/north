use crate::runtime::COORDINATOR;
use crate::guard::coordinator_role::{self, Live, World};
use crate::hook_input::{HookInput, Verdict};
use crate::platform::process;
use std::process::Command;
use std::time::Duration;

const ADOPT_LIMIT: Duration = Duration::from_secs(5);
const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";

pub fn evaluate(input: &HookInput) -> Verdict {
    let window = coordinator_role::is_tom_window(&Live, input).then(|| Live.claude_start()).flatten().and_then(|start| start.conversation);
    adoption(input, window.as_deref(), |session| adopt(session, input.data["cwd"].as_str()))
}

fn adoption(input: &HookInput, window: Option<&str>, adopt: impl FnOnce(&str) -> Option<String>) -> Verdict {
    let session = input.data["session_id"].as_str().unwrap_or("");
    if session.is_empty() || window != Some(session) {
        return Verdict::Allow;
    }
    match adopt(session) {
        Some(notice) if !notice.trim().is_empty() => Verdict::Advise(notice.trim().to_owned()),
        _ => Verdict::Allow,
    }
}

fn adopt(session: &str, cwd: Option<&str>) -> Option<String> {
    let north = std::env::var("NORTH_BIN").unwrap_or_else(|_| "north".to_owned());
    let mut command = Command::new(north);
    command.args(["fleet", "adopt", "--name", COORDINATOR, "--if-free"]).env(CONVERSATION_ENV, session);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = process::run_command_within(command, None, ADOPT_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
