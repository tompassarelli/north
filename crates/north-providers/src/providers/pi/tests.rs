use std::io::{Cursor, Read};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use time::Date;

use crate::fleet::fakes::Paper;
use crate::fleet::model::{Account, Delivery, EntryKind, LaunchRequest, Status, Transcript};
use crate::fleet::prompt::PromptMode;
use crate::fleet::registry::Fleet;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::runtime::{Opening, Runtime};
use crate::fleet::sender::Sender;
use crate::fleet::turns::{TurnExit, TurnSignal};

use super::super::opencode::route::{self, Route, Visibility};
use super::super::opencode::turns;
use super::super::opencode::Deepseek;
use super::PI;
use super::windows::PiWindows;

const FAKE: &str = r#"printf '%s\n' "$*" >> "$(dirname "$0")/args.txt"
printf '%s %s\n' "$PI_CODING_AGENT_DIR" "$(cat "$PI_CODING_AGENT_DIR/models.json")" >> "$(dirname "$0")/env.txt"
case "$*" in
  *"--provider opencode "*) printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[],"usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"stopReason":"error","errorMessage":"429: Free usage limit reached"}}'; exit 0;;
esac
printf '%s\n' '{"type":"session","id":"ses_x"}'
printf '%s\n' '{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"brief"}]}}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"toolCall","name":"bash"}],"usage":{"input":100,"output":10,"cacheRead":40,"cacheWrite":2},"stopReason":"toolUse"}}'
printf '%s\n' '{"type":"tool_execution_start","toolCallId":"c1","toolName":"bash","args":{"command":"echo hi > a.txt"}}'
printf '%s\n' '{"type":"tool_execution_end","toolCallId":"c1","toolName":"bash","result":{},"isError":false}'
printf '%s\n' '{"type":"compaction_end","reason":"threshold","result":{"summary":"s","firstKeptEntryId":"e1","tokensBefore":150000},"aborted":false,"willRetry":false}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done: wrote a.txt"}],"usage":{"input":50,"output":7,"cacheRead":0,"cacheWrite":0},"stopReason":"stop"}}'
printf '%s\n' '{"type":"agent_settled","aborted":false}'
"#;

fn request(cwd: &Path) -> LaunchRequest {
    LaunchRequest {
        verification: false,
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: "worker".into(),
        provider: Deepseek::live(&PI).provider(),
        account: "go".into(),
        cwd: cwd.into(),
        model: None,
        effort: None,
        prompt: "Item: north#1\nBox: add a.txt".into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
        resume: None,
        compact_window: None,
        dispatched: false,
        route: None,
            }
}

fn account(home: &Path) -> Account {
    Account { provider: Deepseek::live(&PI).provider(), label: "go".into(), home: home.into(), token_file: Some(home.join("go.key")), api: None }
}

fn seeing(visibility: Visibility) -> Deepseek {
    let today: Date = route::date("2026-11-10").unwrap();
    Deepseek::reading(&PI, Arc::new(move |_| visibility), Arc::new(move || today))
}

fn argument(argv: &[String], flag: &str) -> Option<String> {
    argv.iter().position(|word| word == flag).and_then(|at| argv.get(at + 1)).cloned()
}

fn run_briefs(runtime: &Deepseek, route: Route, home: &Path, briefs: &[&str]) -> (Transcript, Vec<TurnSignal>, Vec<Value>) {
    let key = home.join("go.key");
    std::fs::write(&key, "key-material\n").unwrap();
    let program: PathBuf = home.join("pi");
    north_base::foundation::shim::write_script(&program, FAKE);
    let host = turns::Host { harness: &PI, route, effort: None, session: "ses_x".into(), scratch: home.join("scratch"), key_file: Some(key), instructions: None, launcher: Vec::new(), program, cwd: home.into() };
    let mut transcript = Transcript { name: "worker".into(), ..Transcript::new() };
    let input: Vec<String> = briefs.iter().enumerate().flat_map(|(at, brief)| runtime.message(&transcript, &format!("u{at}"), brief).unwrap()).collect();
    let mut output = Vec::new();
    turns::serve(&host, Cursor::new(input.join("\n")), &mut output).unwrap();
    let frames: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let mut signals = Vec::new();
    for frame in &frames {
        runtime.observe(frame, &mut transcript);
        signals.extend(runtime.signals(frame));
    }
    (transcript, signals, frames)
}

#[test]
fn a_pi_worker_runs_a_brief_and_reports_through_the_fleet() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Private);

    let spec = runtime.host(&account(home.path()), &request(home.path()), Opening::Fresh(Some("ses_x")), "").unwrap();
    assert_eq!(spec.argv[1], "pi-host");
    assert_eq!(argument(&spec.argv, "--route").as_deref(), Some("go"));
    assert!(spec.argv.iter().all(|word| !word.contains("key-material")), "{:?}", spec.argv);

    let (transcript, signals, _) = run_briefs(&runtime, Route::Go, home.path(), &["Item: north#1\nBox: add a.txt"]);

    let launched = std::fs::read_to_string(home.path().join("args.txt")).unwrap();
    assert!(launched.contains("--mode json ") && launched.contains("--provider opencode-go --model deepseek-v4.1-flash --session-id ses_x -- Item: north#1"), "{launched}");
    let env = std::fs::read_to_string(home.path().join("env.txt")).unwrap();
    let models = format!("{} {{\"providers\":{{\"opencode-go\":{{\"apiKey\":", home.path().join("scratch/agent").display());
    assert!(env.starts_with(&models) && env.contains("\"baseUrl\":\"http://127.0.0.1:") && !env.contains("key-material"), "{env}");
    assert_eq!(transcript.status, Status::Idle);
    assert_eq!(transcript.conversation.as_deref(), Some("ses_x"));
    let kinds: Vec<_> = transcript.entries.iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, [EntryKind::User, EntryKind::Tool, EntryKind::Assistant, EntryKind::TurnEnd], "{:?}", transcript.entries);
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Tool && entry.text == "bash echo hi > a.txt"));
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    let Some(TurnSignal::Ended { exit, totals, .. }) = signals.iter().find(|signal| matches!(signal, TurnSignal::Ended { .. })) else { panic!("{signals:?}") };
    assert_eq!(*exit, TurnExit::Completed);
    let tokens = totals.tokens.unwrap();
    assert_eq!((tokens.input, tokens.output, tokens.cache_read, tokens.cache_write), (150, 17, 40, 2));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Opened { model: Some(model), .. } if model == route::GO_MODEL)));
    a_limited_free_model_runs_the_turn_again_on_paid_go_flash();
    a_pi_window_holds_the_proxy_name_and_counts_as_a_live_fleet_session();
}

fn a_pi_window_holds_the_proxy_name_and_counts_as_a_live_fleet_session() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    let agent = home.path().join(".pi/agent");
    let windows = state.join("pi-windows");
    std::fs::create_dir_all(&windows).unwrap();
    let pid = std::process::id();
    let socket = PathBuf::from(format!("/tmp/north-pi-{pid}.sock"));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::write(
        windows.join(format!("{pid}.json")),
        serde_json::json!({"pid": pid, "conversation": "pi-window", "session_file": "/tmp/pi.jsonl", "socket": socket, "cwd": "/tmp", "started_ms": 1}).to_string(),
    )
    .unwrap();

    let runtime = seeing(Visibility::Private).with_windows(Arc::new(PiWindows::at(state, agent.clone())));
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(runtime)]), Arc::new(Paper::default())).unwrap();
    let record = fleet.adopt("proxy", "pi-window", "/tmp".into()).unwrap();

    assert!(record.adopted());
    assert_eq!(record.conversation.as_deref(), Some("pi-window"));
    assert_eq!(fleet.status(&record), Status::Idle, "dispatch sees the Pi window as a live session");
    assert!(fleet.listens("proxy").unwrap(), "a report to the proxy reaches the Pi window");
    assert!(agent.join("extensions/north-fleet.ts").is_file(), "adopt installs the bridge the window loads");
    assert_eq!(PiWindows::conversation_of(|name| (name == "PI_SESSION_ID").then(|| "pi-window".to_owned())).as_deref(), Some("pi-window"));

    fleet.send_message(&Sender::Session("lead".into()), "proxy", "main is red", Delivery::Queued).unwrap();
    let (mut report, _) = listener.accept().unwrap();
    let mut text = String::new();
    report.read_to_string(&mut text).unwrap();
    assert!(text.contains("main is red"), "the report reaches the Pi window's socket: {text:?}");
}

fn a_limited_free_model_runs_the_turn_again_on_paid_go_flash() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Public);

    let (transcript, signals, frames) = run_briefs(&runtime, Route::Free, home.path(), &["Box: add a.txt"]);

    let starts: Vec<_> = frames.iter().filter(|frame| frame["phase"] == "start").map(|frame| frame["route"].as_str().unwrap()).collect();
    assert_eq!(starts, ["free"]);
    let end = frames.iter().rfind(|frame| frame["phase"] == "end").unwrap();
    assert_eq!((end["tool_calls"].as_u64(), end["compactions"].as_u64()), (Some(1), Some(1)), "{end}");
    assert_eq!(frames.iter().filter(|frame| frame["phase"] == "fallback" && frame["from"] == "free" && frame["to"] == "go").count(), 1);
    let env = std::fs::read_to_string(home.path().join("env.txt")).unwrap();
    assert!(env.lines().next().unwrap().ends_with(r#" {"providers":{"opencode":{"apiKey":"public"}}}"#) && env.lines().nth(1).unwrap().contains(r#"{"providers":{"opencode-go":{"apiKey":"#) && !env.contains("key-material"), "{env}");
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Ended { exit: TurnExit::Completed, .. })));
}

const GROUNDING_NORTH: &str = r#"#!/bin/sh
[ "$2" = prompt-grounding ] || exit 0
cat > "$(dirname "$0")/prompt.json"
printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"Grounding: north#1041 is open"}}'
"#;

const PROMPT_DRIVER: &str = r#"import bridge from "./north-fleet.ts";
const handlers: Record<string, Function> = {};
bridge({ on: (name: string, handler: Function) => { handlers[name] = handler; } } as never);
const ctx = { cwd: "/tmp", sessionManager: { getSessionId: () => "pi-window" } };
console.log(JSON.stringify(await handlers.before_agent_start({ prompt: "what is left on north#1041?" }, ctx)));
"#;

#[test]
fn a_pi_window_prompt_reaches_its_session_with_the_context_north_hook_adds_north_1041() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    north_base::foundation::shim::write_executable(&bin.join("north"), GROUNDING_NORTH);
    std::fs::write(dir.path().join("north-fleet.ts"), super::windows::EXTENSION).unwrap();
    std::fs::write(dir.path().join("driver.ts"), PROMPT_DRIVER).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());

    let bun = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|dir| dir.join("bun")).find(|file| std::fs::File::open(file).and_then(|mut binary| { let mut head = [0; 2]; binary.read_exact(&mut head).map(|()| head != *b"#!") }).unwrap_or(false)).expect("bun on PATH");
    let run = std::process::Command::new(bun).arg("driver.ts").current_dir(dir.path()).env("PATH", path).env("XDG_STATE_HOME", dir.path()).output().unwrap();

    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let reply: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(reply["message"]["content"], "Grounding: north#1041 is open", "the hook's context reaches the Pi session: {reply}");
    let handed: Value = serde_json::from_str(&std::fs::read_to_string(bin.join("prompt.json")).unwrap()).unwrap();
    assert_eq!((handed["hook_event_name"].as_str(), handed["prompt"].as_str(), handed["session_id"].as_str()), (Some("UserPromptSubmit"), Some("what is left on north#1041?"), Some("pi-window")));
}
