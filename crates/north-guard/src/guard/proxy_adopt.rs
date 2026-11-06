use crate::fleet::runtime::PROXY;
use crate::guard::proxy_role::{self, Live};
use crate::hook::{HookInput, Verdict};
use crate::platform::process;
use std::process::Command;
use std::time::Duration;

const ADOPT_LIMIT: Duration = Duration::from_secs(5);
const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    adoption(input, proxy_role::is_tom_window(&Live, input), |session| adopt(session, input.data["cwd"].as_str()))
}

fn adoption(input: &HookInput, window: bool, adopt: impl FnOnce(&str) -> Option<String>) -> Verdict {
    let session = input.data["session_id"].as_str().unwrap_or("");
    if !window || session.is_empty() {
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
    command.args(["fleet", "adopt", "--name", PROXY, "--if-free"]).env(CONVERSATION_ENV, session);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = process::run_command_within(command, None, ADOPT_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_session_start_payload_from_an_interactive_window_runs_the_free_name_adoption_and_no_other_start_does() {
        let payload = br#"{"hook_event_name":"SessionStart","session_id":"conv-1","cwd":"/tmp","source":"startup"}"#;
        let input = HookInput::parse(payload).unwrap();
        let seen = Cell::new(String::new());
        let verdict = adoption(&input, true, |session| {
            seen.set(session.to_owned());
            Some("proxy now reaches this window on account a\n".into())
        });
        assert_eq!(seen.take(), "conv-1");
        assert_eq!(verdict, Verdict::Advise("proxy now reaches this window on account a".into()));

        assert_eq!(adoption(&input, true, |_| Some(String::new())), Verdict::Allow, "a held name prints nothing");
        assert_eq!(adoption(&input, true, |_| None), Verdict::Allow, "a failed adoption leaves the start unchanged");
        let ran = Cell::new(false);
        assert_eq!(adoption(&input, false, |_| { ran.set(true); None }), Verdict::Allow);
        assert!(!ran.get(), "a fleet session or subagent never runs the adoption");
    }
}
