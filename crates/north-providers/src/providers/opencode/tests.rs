use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use time::Date;

use crate::fleet::model::{Account, EntryKind, LaunchRequest, Status, Transcript};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtime::{Opening, Runtime};
use crate::fleet::turns::{TurnExit, TurnSignal};

use super::route::{self, Route, Visibility};
use super::{Opencode, config, turns};

const FAKE: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/args.txt"
model=""; while [ $# -gt 0 ]; do case "$1" in -m) model="$2";; esac; shift; done
case "$model" in
  *free) printf '%s\n' '{"type":"error","sessionID":"ses_x","error":{"type":"provider.free-limit","message":"Free usage limit reached"}}'; exit 0;;
esac
printf '%s\n' '{"type":"step_start","sessionID":"ses_x","part":{}}'
printf '%s\n' '{"type":"tool_use","sessionID":"ses_x","part":{"tool":"read","state":{"input":{"path":"a.txt"}}}}'
printf '%s\n' '{"type":"step_finish","sessionID":"ses_x","part":{"reason":"tool-calls","tokens":{"input":100,"output":10,"reasoning":5,"cache":{"read":40,"write":2}}}}'
printf '%s\n' '{"type":"text","sessionID":"ses_x","part":{"text":"Done: wrote a.txt"}}'
printf '%s\n' '{"type":"step_finish","sessionID":"ses_x","part":{"reason":"stop","tokens":{"input":50,"output":7,"reasoning":0,"cache":{"read":0,"write":0}}}}'
"#;

fn request(cwd: &Path) -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: "worker".into(),
        provider: Opencode::live().provider(),
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
    Account { provider: Opencode::live().provider(), label: "go".into(), home: home.into(), token_file: Some(home.join("go.key")), api: None }
}

fn seeing(visibility: Visibility, today: Date) -> Opencode {
    Opencode::reading(Arc::new(move |_| visibility), Arc::new(move || today)).with_private_lanes()
}

fn day(text: &str) -> Date {
    route::date(text).unwrap()
}

fn fake(directory: &Path) -> PathBuf {
    let program = directory.join("opencode");
    std::fs::write(&program, FAKE).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn argument(argv: &[String], flag: &str) -> Option<String> {
    argv.iter().position(|word| word == flag).and_then(|at| argv.get(at + 1)).cloned()
}

fn run_brief(runtime: &Opencode, route: Route, home: &Path, brief: &str) -> (Transcript, Vec<TurnSignal>, Vec<Value>) {
    let key = home.join("go.key");
    std::fs::write(&key, "key-material\n").unwrap();
    let host = turns::Host { route, session: "ses_x".into(), scratch: home.join("scratch"), key_file: Some(key), launcher: Vec::new(), program: fake(home), cwd: home.into() };
    let mut transcript = Transcript { name: "worker".into(), ..Transcript::new() };
    let input = runtime.message(&transcript, "u1", brief).unwrap().join("\n");
    let mut output = Vec::new();
    turns::serve(&host, Cursor::new(input), &mut output).unwrap();
    let frames: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let mut signals = Vec::new();
    for frame in &frames {
        runtime.observe(frame, &mut transcript);
        signals.extend(runtime.signals(frame));
    }
    (transcript, signals, frames)
}

#[test]
fn an_opencode_worker_runs_a_brief_and_reports_through_the_fleet() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Private, day("2026-11-10"));

    let spec = runtime.host(&account(home.path()), &request(home.path()), Opening::Fresh(Some("ses_x")), "").unwrap();
    assert_eq!(argument(&spec.argv, "--route").as_deref(), Some("go"));
    assert!(spec.argv.iter().all(|word| !word.contains("key-material")), "{:?}", spec.argv);

    let (transcript, signals, _) = run_brief(&runtime, Route::Go, home.path(), "Item: north#1\nBox: add a.txt");

    let launched = std::fs::read_to_string(home.path().join("args.txt")).unwrap();
    assert!(launched.contains("run --standalone ") && launched.contains("-m opencode-go/deepseek-v4.1-flash"), "opencode must run on a private server, not the background service that sits outside the sandbox: {launched}");
    assert_eq!(transcript.status, Status::Idle);
    assert_eq!(transcript.conversation.as_deref(), Some("ses_x"));
    let kinds: Vec<_> = transcript.entries.iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, [EntryKind::User, EntryKind::Tool, EntryKind::Assistant, EntryKind::TurnEnd], "{:?}", transcript.entries);
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    let Some(TurnSignal::Ended { exit, totals, .. }) = signals.iter().find(|signal| matches!(signal, TurnSignal::Ended { .. })) else { panic!("{signals:?}") };
    assert_eq!(*exit, TurnExit::Completed);
    let tokens = totals.tokens.unwrap();
    assert_eq!((tokens.input, tokens.output, tokens.cache_read, tokens.cache_write), (150, 22, 40, 2));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Opened { model: Some(model), .. } if model == route::GO_MODEL)));
    a_free_model_that_is_limited_runs_the_turn_again_on_paid_go_flash();
    a_private_a_public_and_an_unreadable_repo_pick_go_free_and_go();
    a_brief_naming_a_private_repo_or_a_secret_is_refused_for_the_free_model_and_not_for_go();
    the_launched_config_lists_only_the_one_chosen_provider_and_model();
    past_the_zero_retention_date_a_private_lane_launch_is_refused_and_a_public_one_is_admitted();
    the_live_runtime_refuses_a_private_or_unreadable_lane_and_admits_a_public_one();
}

fn a_free_model_that_is_limited_runs_the_turn_again_on_paid_go_flash() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Public, day("2026-11-10"));

    let (transcript, signals, frames) = run_brief(&runtime, Route::Free, home.path(), "Box: add a.txt");

    assert!(frames.iter().any(|frame| frame["phase"] == "fallback" && frame["from"] == "free" && frame["to"] == "go"));
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Ended { exit: TurnExit::Completed, .. })));
}

fn a_private_a_public_and_an_unreadable_repo_pick_go_free_and_go() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let picked = |visibility| {
        let spec = seeing(visibility, day("2026-11-10")).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "").unwrap();
        argument(&spec.argv, "--route").unwrap()
    };

    assert_eq!(picked(Visibility::Private), "go");
    assert_eq!(picked(Visibility::Public), "free");
    assert_eq!(picked(Visibility::Unknown), "go");
    let asked = LaunchRequest { model: Some(route::FREE_MODEL.into()), ..request(home.path()) };
    let refused = seeing(Visibility::Private, day("2026-11-10")).host(&account, &asked, Opening::Fresh(Some("ses_x")), "");
    assert!(refused.is_err_and(|error| error.user_message().contains("fixes this lane's model")));
}

fn a_brief_naming_a_private_repo_or_a_secret_is_refused_for_the_free_model_and_not_for_go() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let launch = |visibility, prompt: &str| seeing(visibility, day("2026-11-10")).host(&account, &LaunchRequest { prompt: prompt.into(), ..request(home.path()) }, Opening::Fresh(Some("ses_x")), "");

    for prompt in ["Copy ~/code/south/main/README.md into the lane", "Follow tompassarelli/south#94", "use sk-ant-api03-abcdefghijklmnopqrstuvwx", "-----BEGIN OPENSSH PRIVATE KEY-----"] {
        assert!(launch(Visibility::Public, prompt).is_err_and(|error| error.user_message().contains("free model may train")), "{prompt}");
        assert!(launch(Visibility::Private, prompt).is_ok(), "{prompt}");
    }
    assert!(launch(Visibility::Public, "Item: north#1\nGo south on the map").is_ok());
}

fn the_launched_config_lists_only_the_one_chosen_provider_and_model() {
    let go = config::content(Route::Go);
    assert_eq!(go["enabled_providers"], json!(["opencode-go"]));
    assert_eq!(go["model"], route::GO_MODEL);
    assert_eq!(go["provider"]["opencode-go"]["options"]["apiKey"], "{env:NORTH_OPENCODE_KEY}");
    let free = config::content(Route::Free);
    assert_eq!(free["enabled_providers"], json!(["opencode"]));
    assert_eq!(free["model"], route::FREE_MODEL);
    assert!(free.get("provider").is_none());
}

fn past_the_zero_retention_date_a_private_lane_launch_is_refused_and_a_public_one_is_admitted() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let after = day("2026-12-01");

    let private = seeing(Visibility::Private, after).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "");
    let public = seeing(Visibility::Public, after).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "");

    assert!(private.is_err_and(|error| error.user_message().contains("zero-retention agreement")));
    assert!(public.is_ok());
    assert!(seeing(Visibility::Private, day("2026-11-30")).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "").is_ok());
}

fn the_live_runtime_refuses_a_private_or_unreadable_lane_and_admits_a_public_one() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let live = |visibility| Opencode::reading(Arc::new(move |_| visibility), Arc::new(|| day("2026-11-10"))).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "");

    assert!(live(Visibility::Private).is_err_and(|error| error.user_message().contains("public-repository lanes only")));
    assert!(live(Visibility::Unknown).is_err_and(|error| error.user_message().contains("public-repository lanes only")));
    assert!(live(Visibility::Public).is_ok());
}
