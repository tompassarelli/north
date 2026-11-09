use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use time::Date;

use crate::fleet::model::{Account, EntryKind, LaunchRequest, Status, Transcript};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtime::{Opening, Runtime};
use crate::fleet::turns::{TurnExit, TurnSignal};

use super::super::opencode::route::{self, Route, Visibility};
use super::super::opencode::turns;
use super::super::opencode::Deepseek;
use super::PI;

const FAKE: &str = r#"printf '%s\n' "$*" >> "$(dirname "$0")/args.txt"
printf '%s %s\n' "$PI_CODING_AGENT_DIR" "$OPENCODE_API_KEY" >> "$(dirname "$0")/env.txt"
case "$*" in
  *"--provider opencode "*) printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[],"usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"stopReason":"error","errorMessage":"429: Free usage limit reached"}}'; exit 0;;
esac
printf '%s\n' '{"type":"session","id":"ses_x"}'
printf '%s\n' '{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"brief"}]}}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"toolCall","name":"bash"}],"usage":{"input":100,"output":10,"cacheRead":40,"cacheWrite":2},"stopReason":"toolUse"}}'
printf '%s\n' '{"type":"tool_execution_start","toolCallId":"c1","toolName":"bash","args":{"command":"echo hi > a.txt"}}'
printf '%s\n' '{"type":"tool_execution_end","toolCallId":"c1","toolName":"bash","result":{},"isError":false}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done: wrote a.txt"}],"usage":{"input":50,"output":7,"cacheRead":0,"cacheWrite":0},"stopReason":"stop"}}'
printf '%s\n' '{"type":"agent_settled","aborted":false}'
"#;

fn request(cwd: &Path) -> LaunchRequest {
    LaunchRequest {
        verification: false,
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
        slots: Default::default(),
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
    let host = turns::Host { harness: &PI, route, session: "ses_x".into(), scratch: home.join("scratch"), key_file: Some(key), instructions: None, launcher: Vec::new(), program, cwd: home.into() };
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
    assert_eq!(env.trim(), format!("{} key-material", home.path().join("scratch/agent").display()));
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
}

fn a_limited_free_model_runs_the_turn_again_on_paid_go_flash() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Public);

    let (transcript, signals, frames) = run_briefs(&runtime, Route::Free, home.path(), &["Box: add a.txt"]);

    let starts: Vec<_> = frames.iter().filter(|frame| frame["phase"] == "start").map(|frame| frame["route"].as_str().unwrap()).collect();
    assert_eq!(starts, ["free"]);
    assert_eq!(frames.iter().filter(|frame| frame["phase"] == "fallback" && frame["from"] == "free" && frame["to"] == "go").count(), 1);
    let env = std::fs::read_to_string(home.path().join("env.txt")).unwrap();
    assert!(env.lines().next().unwrap().ends_with(" public") && env.lines().nth(1).unwrap().ends_with(" key-material"), "{env}");
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Ended { exit: TurnExit::Completed, .. })));
}
