use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

use crate::claude_stream::{
    DeliveryState, StreamFrame, decode_frame, encode_initialize, encode_interrupt,
    encode_user_message,
};
use crate::error::{NorthError, NorthResult};

pub const INITIALIZE_ID: &str = "init1";
pub const INTERRUPT_ID: &str = "int1";
pub const RUNTIME_DEADLINE: Duration = Duration::from_secs(60);
pub const RUNTIME_FLAGS: [&str; 11] = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--safe-mode",
    "--model",
    "haiku",
    "--permission-mode",
    "bypassPermissions",
];
const FIRST_MESSAGE: &str = "Reply with exactly the word OK and nothing else.";
// Claude Code refuses a standalone foreground `sleep`, so the long-running task must be some other command.
const SECOND_MESSAGE: &str = "Run exactly this bash command and nothing else: tail -f /dev/null";

const USAGE: &str = "Usage: north contract-probe --validate FIXTURE.jsonl | --runtime PATH";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKind {
    Initialize,
    Interrupt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expectation {
    pub requests: Vec<(String, ControlKind)>,
    pub messages: Vec<(String, Option<DeliveryState>)>,
}

impl Expectation {
    pub fn for_fixture(frames: &[StreamFrame]) -> Self {
        let mut messages: Vec<(String, Option<DeliveryState>)> = Vec::new();
        for frame in frames {
            let uuid = match frame {
                StreamFrame::CommandLifecycle(c) => Some(&c.command_uuid),
                StreamFrame::User(u) if u.is_replay => u.uuid.as_ref(),
                _ => None,
            };
            if let Some(uuid) = uuid.filter(|uuid| messages.iter().all(|(seen, _)| seen != *uuid)) {
                messages.push((uuid.clone(), None));
            }
        }
        Self { requests: probe_requests(), messages }
    }

    pub fn for_runtime(first: &str, second: &str) -> Self {
        Self {
            requests: probe_requests(),
            messages: vec![
                (first.into(), Some(DeliveryState::Completed)),
                (second.into(), Some(DeliveryState::Cancelled)),
            ],
        }
    }
}

fn probe_requests() -> Vec<(String, ControlKind)> {
    vec![
        (INITIALIZE_ID.into(), ControlKind::Initialize),
        (INTERRUPT_ID.into(), ControlKind::Interrupt),
    ]
}

pub fn decode_capture(text: &str) -> NorthResult<Vec<StreamFrame>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| decode_frame(line).map_err(NorthError::from))
        .collect()
}

pub fn validate(frames: &[StreamFrame], expect: &Expectation) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();
    check_lifecycles(frames, expect, &mut violations);
    check_control(frames, expect, &mut violations);
    check_interrupted_turn(frames, expect, &mut violations);
    if violations.is_empty() { Ok(()) } else { Err(violations) }
}

fn check_lifecycles(frames: &[StreamFrame], expect: &Expectation, violations: &mut Vec<String>) {
    if expect.messages.is_empty() {
        violations.push("no user message was observed".into());
    }
    for (uuid, terminal) in &expect.messages {
        let states: Vec<DeliveryState> = frames
            .iter()
            .filter_map(|frame| match frame {
                StreamFrame::CommandLifecycle(c) if &c.command_uuid == uuid => Some(c.state),
                _ => None,
            })
            .collect();
        let ends = match (states.get(2), terminal) {
            (Some(last), Some(terminal)) => last == terminal,
            (Some(last), None) => matches!(last, DeliveryState::Completed | DeliveryState::Cancelled),
            (None, _) => false,
        };
        if states.len() != 3
            || states[..2] != [DeliveryState::Queued, DeliveryState::AcceptedIntoTurn]
            || !ends
        {
            violations.push(format!("message {uuid} went {states:?}, not queued → started → terminal"));
        }
    }
}

fn check_control(frames: &[StreamFrame], expect: &Expectation, violations: &mut Vec<String>) {
    let mut answered: HashMap<&str, usize> = HashMap::new();
    for frame in frames {
        let StreamFrame::ControlResponse(response) = frame else { continue };
        let Some((id, kind)) = expect.requests.iter().find(|(id, _)| *id == response.request_id) else {
            violations.push(format!("control_response {} answers no request", response.request_id));
            continue;
        };
        *answered.entry(id).or_default() += 1;
        let body = response.response.as_ref();
        let shaped = match kind {
            ControlKind::Initialize => body.is_some_and(|b| b.get("claude_code_version").is_some()),
            ControlKind::Interrupt => body.is_some_and(|b| b.get("still_queued").is_some()),
        };
        if response.subtype != "success" || !shaped {
            violations.push(format!("control_response {id} does not answer {kind:?}"));
        }
    }
    for (id, kind) in &expect.requests {
        let count = answered.get(id.as_str()).copied().unwrap_or(0);
        if count != 1 {
            violations.push(format!("{kind:?} request {id} got {count} control_responses"));
        }
    }
}

fn check_interrupted_turn(frames: &[StreamFrame], expect: &Expectation, violations: &mut Vec<String>) {
    let last = frames.iter().rev().find_map(|frame| match frame {
        StreamFrame::Result(result) => Some(result),
        _ => None,
    });
    let Some(last) = last else {
        violations.push("no result frame".into());
        return;
    };
    if !last.terminal_reason.as_deref().is_some_and(|reason| reason.starts_with("aborted")) {
        violations.push(format!("final result ended {:?}, not aborted", last.terminal_reason));
    }
    if let Some((uuid, _)) = expect.messages.last()
        && !last.user_message_uuids.contains(uuid)
    {
        violations.push(format!("final result does not belong to interrupted message {uuid}"));
    }
}

pub async fn run(arguments: &[String]) -> NorthResult<u8> {
    let report = |outcome: Result<(), Vec<String>>| match outcome {
        Ok(()) => {
            println!("contract-probe: ok");
            0
        }
        Err(violations) => {
            for violation in violations {
                eprintln!("contract-probe: {violation}");
            }
            1
        }
    };
    match arguments {
        [mode, fixture] if mode == "--validate" => {
            let frames = decode_capture(&fs::read_to_string(fixture)?)?;
            Ok(report(validate(&frames, &Expectation::for_fixture(&frames))))
        }
        [mode, path] if mode == "--runtime" => {
            let (frames, expect) = probe_runtime(std::slice::from_ref(path), RUNTIME_DEADLINE).await?;
            Ok(report(validate(&frames, &expect)))
        }
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

pub async fn probe_runtime(command: &[String], deadline: Duration) -> NorthResult<(Vec<StreamFrame>, Expectation)> {
    let directory = tempfile::tempdir()?;
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .args(RUNTIME_FLAGS)
        .current_dir(directory.path())
        .env("DISABLE_AUTOUPDATER", "1")
        .env("DISABLE_INSTALLATION_CHECKS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let missing = || NorthError::State("runtime pipe was not captured".into());
    let mut session = Session {
        input: child.stdin.take().ok_or_else(missing)?,
        output: BufReader::new(child.stdout.take().ok_or_else(missing)?).lines(),
        frames: Vec::new(),
    };
    let (first, second) = (new_uuid()?, new_uuid()?);
    let until = Instant::now() + deadline;
    let conversation = async {
        session.send(&encode_initialize(INITIALIZE_ID)).await?;
        session.until(|f| matches!(f, StreamFrame::ControlResponse(c) if c.request_id == INITIALIZE_ID)).await?;
        session.send(&encode_user_message(&first, FIRST_MESSAGE)).await?;
        session.until(|f| terminal(f, &first)).await?;
        session.send(&encode_user_message(&second, SECOND_MESSAGE)).await?;
        session.until(|f| matches!(f, StreamFrame::TaskStarted(_))).await?;
        session.send(&encode_interrupt(INTERRUPT_ID)).await?;
        session.until(|f| terminal(f, &second)).await?;
        Ok::<_, NorthError>(())
    };
    let outcome = timeout_at(until, conversation).await;
    drop(session.input);
    let drained = timeout_at(until, async {
        while let Some(line) = session.output.next_line().await? {
            session.frames.push(decode_frame(&line)?);
        }
        Ok::<_, NorthError>(())
    })
    .await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    match (outcome, drained) {
        (Ok(Ok(())), Ok(Ok(()))) => Ok((session.frames, Expectation::for_runtime(&first, &second))),
        (Ok(Err(error)), _) | (_, Ok(Err(error))) => Err(error),
        _ => Err(NorthError::Protocol(format!("runtime did not finish the probe within {deadline:?}"))),
    }
}

struct Session {
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
    frames: Vec<StreamFrame>,
}

impl Session {
    async fn send(&mut self, line: &str) -> NorthResult<()> {
        self.input.write_all(format!("{line}\n").as_bytes()).await?;
        Ok(self.input.flush().await?)
    }

    async fn until(&mut self, done: impl Fn(&StreamFrame) -> bool) -> NorthResult<()> {
        while let Some(line) = self.output.next_line().await? {
            let frame = decode_frame(&line)?;
            let finished = done(&frame);
            self.frames.push(frame);
            if finished {
                return Ok(());
            }
        }
        Err(NorthError::Protocol("runtime closed its stream mid-probe".into()))
    }
}

fn terminal(frame: &StreamFrame, uuid: &str) -> bool {
    matches!(
        frame,
        StreamFrame::CommandLifecycle(c)
            if c.command_uuid == uuid
                && matches!(c.state, DeliveryState::Completed | DeliveryState::Cancelled)
    )
}

fn new_uuid() -> NorthResult<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERRUPT: &str = include_str!("fixtures/claude-stream/interrupt.jsonl");
    const MESSAGE: &str = "55519047-b95b-4d1f-b79b-51e0ecd3b556";
    const TOOL_RESULT: &str = "5a598f27-ad02-46a1-b28d-ebfd5b03dbcb";

    fn check(fixture: &str) -> Result<(), Vec<String>> {
        let frames = decode_capture(fixture).unwrap();
        validate(&frames, &Expectation::for_fixture(&frames))
    }

    fn swap(text: &str, a: &str, b: &str) -> String {
        text.replace(a, "\u{0}").replace(b, a).replace('\u{0}', b)
    }

    #[test]
    fn contract_probe_validates_the_interrupt_fixture() {
        assert_eq!(check(INTERRUPT), Ok(()));
    }

    #[test]
    fn contract_probe_rejects_a_stream_without_lifecycle_frames() {
        let stripped: String = INTERRUPT
            .lines()
            .filter(|line| !line.contains(r#""type":"command_lifecycle""#))
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(check(&stripped).is_err());
    }

    #[test]
    fn contract_probe_rejects_a_result_for_a_swapped_uuid() {
        let lines: Vec<&str> = INTERRUPT.lines().collect();
        let swapped: String = lines
            .iter()
            .map(|line| {
                let swapped = if line.contains(r#""type":"result""#) || line.contains(r#""tool_result""#) {
                    swap(line, MESSAGE, TOOL_RESULT)
                } else {
                    line.to_string()
                };
                format!("{swapped}\n")
            })
            .collect();
        assert_ne!(swapped, INTERRUPT);
        assert!(check(&swapped).is_err());
    }

    #[test]
    fn contract_probe_rejects_swapped_control_request_ids() {
        assert!(check(&swap(INTERRUPT, r#""init1""#, r#""int1""#)).is_err());
    }

    #[test]
    fn contract_probe_rejects_a_turn_that_was_not_aborted() {
        assert!(check(&INTERRUPT.replace(r#""aborted_tools""#, r#""completed""#)).is_err());
    }

    #[test]
    fn contract_probe_accepts_an_interrupt_that_landed_mid_stream() {
        assert_eq!(check(&INTERRUPT.replace(r#""aborted_tools""#, r#""aborted_streaming""#)), Ok(()));
    }

    #[tokio::test]
    async fn contract_probe_runtime_drives_message_then_interrupt() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("claude");
        fs::write(&script, STUB).unwrap();
        let command = vec!["sh".to_string(), script.display().to_string()];
        let (frames, expect) = probe_runtime(&command, Duration::from_secs(10)).await.unwrap();
        assert_eq!(validate(&frames, &expect), Ok(()));
    }

    const STUB: &str = r#"
case " $* " in *" --safe-mode --model haiku "*) ;; *) exit 3 ;; esac
n=0
field() { printf '%s' "$2" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }
life() { printf '{"type":"command_lifecycle","command_uuid":"%s","state":"%s"}\n' "$1" "$2"; }
while IFS= read -r line; do
  case "$line" in
    *'"subtype":"initialize"'*)
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"claude_code_version":"9.9.9"}}}\n' "$(field request_id "$line")" ;;
    *'"subtype":"interrupt"'*)
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"still_queued":[]}}}\n' "$(field request_id "$line")"
      printf '{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_tools","user_message_uuids":["%s"]}\n' "$second"
      life "$second" cancelled ;;
    *'"type":"user"'*)
      uuid="$(field uuid "$line")"; n=$((n + 1))
      life "$uuid" queued; life "$uuid" started
      printf '{"type":"user","uuid":"%s","isReplay":true,"message":{"role":"user","content":"x"}}\n' "$uuid"
      if [ "$n" = 1 ]; then
        printf '{"type":"result","subtype":"success","terminal_reason":"completed","user_message_uuids":["%s"]}\n' "$uuid"
        life "$uuid" completed
      else
        second="$uuid"
        printf '{"type":"system","subtype":"task_started","task_id":"t1"}\n'
      fi ;;
  esac
done
"#;
}
