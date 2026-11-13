use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use time::Date;

use crate::fleet::model::{Account, Effort, EntryKind, LaunchRequest, Machine, Status, Transcript, UsageLimit};
use crate::fleet::prompt::PromptMode;
use crate::fleet::runtime::{Opening, Runtime};
use crate::fleet::turns::{TurnExit, TurnSignal};

use super::route::{self, Route, Visibility};
use super::harness::{Harness, OPENCODE};
use super::overlay::Arm;
use super::relay::{self, Relay};
use super::{Deepseek, config, turns};

const FAKE: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/args.txt"
printf '%s\n' "$OPENCODE_CONFIG_CONTENT" > "$(dirname "$0")/config.json"
case "$*" in
  *go-limit*) printf '%s\n' '{"type":"error","sessionID":"ses_x","error":{"type":"provider.go-limit","message":"OpenCode Go usage limit reached. Switch to a free model, or turn on Extra Usage on the Go page in OpenCode Console to keep going.","response":{"body":"{\"resetsAt\":\"2026-11-14T03:00:00Z\"}"}}}'; exit 0;;
esac
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
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: "worker".into(),
        provider: Deepseek::opencode().provider(),
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
    Account { provider: Deepseek::opencode().provider(), label: "go".into(), home: home.into(), token_file: Some(home.join("go.key")), api: None }
}

fn seeing(visibility: Visibility, today: Date) -> Deepseek {
    Deepseek::reading(&OPENCODE, Arc::new(move |_| visibility), Arc::new(move || today))
}

fn day(text: &str) -> Date {
    route::date(text).unwrap()
}

fn fake(directory: &Path) -> PathBuf {
    let program = directory.join("opencode");
    north_base::foundation::shim::write_script(&program, FAKE.trim_start_matches("#!/bin/sh\n"));
    program
}

fn argument(argv: &[String], flag: &str) -> Option<String> {
    argv.iter().position(|word| word == flag).and_then(|at| argv.get(at + 1)).cloned()
}

fn run_briefs(runtime: &Deepseek, route: Route, effort: Option<&str>, home: &Path, briefs: &[&str]) -> (Transcript, Vec<TurnSignal>, Vec<Value>) {
    let key = home.join("go.key");
    std::fs::write(&key, "key-material\n").unwrap();
    let host = turns::Host { harness: &OPENCODE, route, effort: effort.map(str::to_owned), session: "ses_x".into(), scratch: home.join("scratch"), key_file: Some(key), instructions: None, launcher: Vec::new(), program: fake(home), cwd: home.into() };
    let mut transcript = Transcript { name: "worker".into(), ..Transcript::new() };
    let input: Vec<String> = briefs.iter().enumerate().flat_map(|(at, brief)| runtime.message(&transcript, &format!("u{at}"), brief).unwrap()).collect();
    let input = input.join("\n");
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
    let drawn = crate::fleet::registry::efforts::taken(&runtime, None, None, "worker").unwrap().unwrap();
    assert!(matches!(drawn, Effort::High | Effort::Max), "{drawn:?}");
    let armed = runtime.host(&account(home.path()), &LaunchRequest { effort: Some(drawn), ..request(home.path()) }, Opening::Fresh(Some("ses_x")), "").unwrap();
    assert_eq!(argument(&armed.argv, "--effort").as_deref(), Some(drawn.label()));
    let refused = runtime.host(&account(home.path()), &LaunchRequest { effort: Some(Effort::Medium), ..request(home.path()) }, Opening::Fresh(Some("ses_x")), "");
    let why = refused.err().map(|error| error.user_message()).unwrap_or_default();
    assert!(why.contains("runs only at high or max effort"), "{why}");

    let (transcript, signals, _) = run_briefs(&runtime, Route::Go, None, home.path(), &["Item: north#1\nBox: add a.txt"]);

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
    let armed_home = tempfile::tempdir().unwrap();
    let (_, armed_signals, _) = run_briefs(&runtime, Route::Go, Some(drawn.label()), armed_home.path(), &["Item: north#1\nBox: add a.txt"]);
    let armed_launch = std::fs::read_to_string(armed_home.path().join("args.txt")).unwrap();
    assert!(armed_launch.contains(&format!("-m {}#{}", route::GO_MODEL, drawn.label())), "{armed_launch}");
    assert!(armed_signals.iter().any(|signal| matches!(signal, TurnSignal::Opened { effort: Some(effort), .. } if effort == drawn.label())), "{armed_signals:?}");
    let prompt = signals.iter().position(|signal| matches!(signal, TurnSignal::Prompt { .. })).unwrap();
    let ended = signals.iter().position(|signal| matches!(signal, TurnSignal::Ended { .. })).unwrap();
    assert!(signals[prompt + 1..ended].iter().any(|signal| matches!(signal, TurnSignal::Started { .. })), "a launch waits for a signal after the prompt, and a turn over 120 s ended its own session when only the turn's end gave one: {signals:?}");
    a_limited_free_model_runs_the_turn_again_on_go_and_a_go_limit_ends_the_turn_as_a_usage_limit();
    a_private_a_public_and_an_unreadable_repo_pick_go_and_only_a_public_one_asking_for_the_free_model_gets_it();
    a_brief_naming_a_private_repo_or_a_secret_is_refused_for_the_free_model_and_not_for_go();
    the_launched_config_lists_only_the_one_chosen_provider_and_model();
    past_the_zero_retention_date_a_private_lane_launch_is_refused_and_a_public_one_is_admitted();
}

fn a_limited_free_model_runs_the_turn_again_on_go_and_a_go_limit_ends_the_turn_as_a_usage_limit() {
    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Public, day("2026-11-10"));

    let (transcript, signals, frames) = run_briefs(&runtime, Route::Free, None, home.path(), &["Box: add a.txt", "Box: add b.txt"]);

    let falls = frames.iter().filter(|frame| frame["phase"] == "fallback" && frame["from"] == "free" && frame["to"] == "go").count();
    assert_eq!(falls, 1, "a lane that found the free model limited stays on Go instead of trying free again each turn");
    let starts: Vec<_> = frames.iter().filter(|frame| frame["phase"] == "start").map(|frame| frame["route"].as_str().unwrap()).collect();
    assert_eq!(starts, ["free", "go"]);
    let launched = std::fs::read_to_string(home.path().join("args.txt")).unwrap();
    assert_eq!(launched.lines().filter(|line| line.contains("-free")).count(), 1, "{launched}");
    assert!(transcript.entries.iter().any(|entry| entry.kind == EntryKind::Assistant && entry.text == "Done: wrote a.txt"));
    assert!(signals.iter().any(|signal| matches!(signal, TurnSignal::Ended { exit: TurnExit::Completed, .. })));

    let limited = tempfile::tempdir().unwrap();
    let (transcript, _, frames) = run_briefs(&runtime, Route::Go, None, limited.path(), &["Box: go-limit"]);
    let end = frames.iter().find(|frame| frame["phase"] == "end").unwrap();
    assert_eq!((end["exit"].as_str(), end["limit"]["resets_at_s"].as_u64()), (Some("failed"), Some(1_794_625_200)), "{end}");
    assert!(frames.iter().all(|frame| frame["phase"] != "fallback"), "a Go limit has no paid route to fall to");
    let message = "OpenCode Go usage limit reached. Switch to a free model, or turn on Extra Usage on the Go page in OpenCode Console to keep going.";
    assert_eq!(transcript.limit, Some(UsageLimit { resets_at_s: Some(1_794_625_200), message: Some(message.to_owned()) }), "the Go plan's limit reads as a usage limit, which parks the session, not as a generic provider error");
    assert_eq!(runtime.quota(end).map(|quota| quota.windows.iter().map(|window| window.used_percent).collect::<Vec<_>>()), Some(vec![100]));
}

fn a_private_a_public_and_an_unreadable_repo_pick_go_and_only_a_public_one_asking_for_the_free_model_gets_it() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let picked = |visibility| {
        let spec = seeing(visibility, day("2026-11-10")).host(&account, &request(home.path()), Opening::Fresh(Some("ses_x")), "").unwrap();
        argument(&spec.argv, "--route").unwrap()
    };

    assert_eq!(picked(Visibility::Private), "go");
    assert_eq!(picked(Visibility::Public), "go");
    let asked_free = LaunchRequest { model: Some(route::FREE_MODEL.into()), ..request(home.path()) };
    let spec = seeing(Visibility::Public, day("2026-11-10")).host(&account, &asked_free, Opening::Fresh(Some("ses_x")), "").unwrap();
    assert_eq!(argument(&spec.argv, "--route").as_deref(), Some("free"));
    let asked_go = LaunchRequest { model: Some(route::GO_MODEL.into()), ..request(home.path()) };
    assert!(seeing(Visibility::Public, day("2026-11-10")).host(&account, &asked_go, Opening::Fresh(Some("ses_x")), "").is_ok());
    assert_eq!(picked(Visibility::Unknown), "go");
    let delivered = |visibility| {
        let asked = LaunchRequest { model: (visibility == Visibility::Public).then(|| route::FREE_MODEL.into()), ..request(home.path()) };
        let spec = seeing(visibility, day("2026-11-10")).host(&account, &asked, Opening::Fresh(Some("ses_x")), "LIVE-ORDER-OF-THE-OPERATOR").unwrap();
        std::fs::read_to_string(argument(&spec.argv, "--instructions").unwrap()).unwrap()
    };
    let (public, private) = (delivered(Visibility::Public), delivered(Visibility::Private));
    assert!(!public.contains("LIVE-ORDER-OF-THE-OPERATOR") && public.contains("Done:"), "{public}");
    assert!(private.contains("LIVE-ORDER-OF-THE-OPERATOR"));
    let asked = LaunchRequest { model: Some(route::FREE_MODEL.into()), ..request(home.path()) };
    let refused = seeing(Visibility::Private, day("2026-11-10")).host(&account, &asked, Opening::Fresh(Some("ses_x")), "");
    assert!(refused.is_err_and(|error| error.user_message().contains("fixes this lane's model")));
    let naming_private = LaunchRequest { prompt: "Follow tompassarelli/south#94".into(), ..request(home.path()) };
    let on_go = |today| seeing(Visibility::Public, day(today)).host(&account, &naming_private, Opening::Fresh(Some("ses_x")), "");
    assert!(on_go("2026-12-01").is_err_and(|error| error.user_message().contains("Go plan's zero-retention agreement has ended")));
    assert!(on_go("2026-11-30").is_ok());
}

fn a_brief_naming_a_private_repo_or_a_secret_is_refused_for_the_free_model_and_not_for_go() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let launch = |visibility, prompt: &str| seeing(visibility, day("2026-11-10")).host(&account, &LaunchRequest { prompt: prompt.into(), model: (visibility == Visibility::Public).then(|| route::FREE_MODEL.into()), ..request(home.path()) }, Opening::Fresh(Some("ses_x")), "");

    for prompt in ["Copy ~/code/south/main/README.md into the lane", "Follow tompassarelli/south#94", "use sk-ant-api03-abcdefghijklmnopqrstuvwx", "-----BEGIN OPENSSH PRIVATE KEY-----"] {
        assert!(launch(Visibility::Public, prompt).is_err_and(|error| error.user_message().contains("free model may train")), "{prompt}");
        assert!(launch(Visibility::Private, prompt).is_ok(), "{prompt}");
    }
    assert!(launch(Visibility::Public, "Item: north#1\nGo south on the map").is_ok());
    assert!(launch(Visibility::Public, "ask-before-cost-or-exposure and task-ledger-entries-keep-their-order").is_ok());
}

fn the_launched_config_lists_only_the_one_chosen_provider_and_model() {
    let relay = Relay::start(Path::new("/nonexistent/go.key")).unwrap();
    let go = config::content(Route::Go, Some(&relay));
    assert_eq!(go["enabled_providers"], json!(["opencode-go"]));
    assert_eq!(go["model"], route::GO_MODEL);
    assert_eq!(go["provider"]["opencode-go"]["options"], json!({"baseURL": relay.api(), "apiKey": relay.token()}));
    let free = config::content(Route::Free, None);
    assert_eq!(free["enabled_providers"], json!(["opencode"]));
    assert_eq!(free["model"], route::FREE_MODEL);
    assert!(free.get("provider").is_none());
}

#[test]
fn a_child_the_client_starts_sees_no_go_key() {
    let guarded = |command: &str| north_base::shell::started(command, &crate::fleet::claude_only::DEEPSEEK_PROVIDERS);

    let home = tempfile::tempdir().unwrap();
    let runtime = seeing(Visibility::Private, day("2026-11-10"));
    let spec = runtime.host(&account(home.path()), &request(home.path()), Opening::Fresh(Some("ses_x")), "").unwrap();
    assert_eq!(argument(&spec.argv, "--route").as_deref(), Some("go"), "a private lane launches on the paid Go model");

    run_briefs(&runtime, Route::Go, None, home.path(), &["Item: north#1\nBox: add a.txt"]);

    let launched: Value = serde_json::from_str(&std::fs::read_to_string(home.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(launched["enabled_providers"], json!([route::GO_PROVIDER]), "only the chosen provider is enabled: {launched}");
    assert_eq!(launched["model"], route::GO_MODEL);
    assert_eq!(launched["small_model"], route::GO_MODEL, "the small model is the same one, so no cheap step slides onto another model: {launched}");
    let providers: Vec<&str> = launched["provider"].as_object().map(|providers| providers.keys().map(String::as_str).collect()).unwrap_or_default();
    assert_eq!(providers, [route::GO_PROVIDER], "the config names no provider besides the chosen one: {launched}");

    for harness in [&OPENCODE as &'static dyn Harness, &crate::providers::pi::PI] {
        let home = tempfile::tempdir().unwrap();
        let key = home.path().join("go.key");
        std::fs::write(&key, "key-material\n").unwrap();
        let program = home.path().join("client");
        north_base::foundation::shim::write_script(&program, "sh -c env > \"$(dirname \"$0\")/child.env\"\n");
        let host = turns::Host { harness, route: Route::Go, effort: None, session: "ses_x".into(), scratch: home.path().join("scratch"), key_file: Some(key), instructions: None, launcher: Vec::new(), program, cwd: home.path().into() };
        turns::serve(&host, Cursor::new(r#"{"type":"user","uuid":"u0","text":"hi"}"#), &mut Vec::new()).unwrap();
        let child = std::fs::read_to_string(home.path().join("child.env")).unwrap();
        let held: Vec<&str> = child.lines().filter(|line| line.starts_with(&format!("{}=", config::KEY_ENV)) || line.starts_with(&format!("{}=", crate::providers::pi::KEY_ENV)) || line.contains("key-material")).map(|line| line.split('=').next().unwrap_or_default()).collect();
        assert!(held.is_empty(), "a child the {} client started holds the Go key in {held:?}", harness.provider());
    }

    let head = |text: &str| relay::read_request(&mut Cursor::new(text.to_owned())).unwrap();
    let asked = head("POST /inference/go/openai/v1/chat/completions HTTP/1.1\r\n@/home/a/notes: x\r\nX-Title: a\r\nAuthorization: Bearer t\r\n\r\n").unwrap();
    assert_eq!((asked.headers, asked.authorization.as_str()), (vec!["X-Title: a".to_owned()], "Bearer t"), "a header named like @FILE would make curl read FILE outside the sandbox");
    for outside in ["GET /zen/go/v1/usage HTTP/1.1", "GET /inference/go/openai/v1/../../../zen/go/v1/usage HTTP/1.1", "GET @/home/a/notes HTTP/1.1", "GET /inference/go/openai/v1/.%2e/.%2e/zen HTTP/1.1", "GET /inference/go/openai/v1/{a,b} HTTP/1.1"] {
        assert!(head(&format!("{outside}\r\n\r\n")).is_none(), "the relay sends the paid key outside Go's API: {outside}");
    }
    assert!(head(&format!("POST /inference/go/openai/v1/chat/completions HTTP/1.1\r\nX-Pad: {}\r\n\r\n", "a".repeat(70_000))).is_none(), "a request head without end must be refused, not buffered");
    let gate = Relay::start(Path::new("/nonexistent/go.key")).unwrap();
    let ask = |request: String| {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(gate.api().trim_start_matches("http://").split('/').next().unwrap()).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer.lines().next().unwrap_or_default().to_owned()
    };
    assert_eq!(ask(format!("POST /inference/go/openai/v1/chat/completions HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Length: 18446744073709551615\r\n\r\n{{}}", gate.token())), "HTTP/1.1 413 Content Too Large", "an oversize declared length must be refused before the relay allocates or reads it");
    assert_eq!(ask(format!("GET /zen/go/v1/usage HTTP/1.1\r\nAuthorization: Bearer {}\r\n\r\n", gate.token())), "HTTP/1.1 400 Bad Request");
    assert_eq!(ask("POST /inference/go/openai/v1/chat/completions HTTP/1.1\r\nAuthorization: Bearer wrong\r\n\r\n".to_owned()), "HTTP/1.1 401 Unauthorized");

    for command in ["opencode run -m opencode-go/deepseek-v4.1-flash hi", "cd lane && opencode run hi", "env FOO=1 /usr/bin/opencode run hi", "bash -c 'opencode run hi'"] {
        assert_eq!(guarded(command).as_deref(), Some("opencode"), "{command}");
    }
    for command in ["opencode --version", "opencode models", "rg opencode docs", "echo \"opencode run\"", "north fleet launch --provider opencode --name w"] {
        assert_eq!(guarded(command), None, "{command}");
    }
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

#[test]
fn a_deepseek_brief_carries_the_overlay_on_its_arm_and_not_on_the_control() {
    let home = tempfile::tempdir().unwrap();
    let account = account(home.path());
    let delivery = |harness: &'static dyn Harness, name: &str| {
        let asked = LaunchRequest { name: name.into(), ..request(home.path()) };
        let spec = Deepseek::reading(harness, Arc::new(|_| Visibility::Private), Arc::new(|| day("2026-11-10"))).host(&account, &asked, Opening::Fresh(Some("ses_x")), "LIVE-ORDER-OF-THE-OPERATOR").unwrap();
        std::fs::read_to_string(argument(&spec.argv, "--instructions").unwrap()).unwrap()
    };
    let harnesses: [&'static dyn Harness; 2] = [&OPENCODE, &crate::providers::pi::PI];
    let mut seen = [false; 2];
    for harness in harnesses {
        for index in 0..8 {
            let name = format!("worker-{index}");
            let arm = Arm::of(&name);
            let brief = delivery(harness, &name);
            assert_eq!(brief.contains("# DeepSeek overlay"), arm == Arm::Overlay, "{} {name} carried the wrong brief", harness.provider());
            assert!(brief.contains("LIVE-ORDER-OF-THE-OPERATOR"), "{} {name} lost the standing orders", harness.provider());
            seen[if arm == Arm::Overlay { 1 } else { 0 }] = true;
        }
    }
    assert_eq!(seen, [true, true], "one arm drew every session");
    let drawing = Deepseek::reading(&OPENCODE, Arc::new(|_| Visibility::Private), Arc::new(|| day("2026-11-10")));
    let crossed: std::collections::BTreeSet<(bool, &str)> = (0..16)
        .map(|index| format!("worker-{index}"))
        .map(|name| (Arm::of(&name) == Arm::Overlay, crate::fleet::registry::efforts::taken(&drawing, None, None, &name).unwrap().unwrap().label()))
        .collect();
    assert_eq!(crossed.len(), 4, "the effort a name draws follows its overlay arm, so the two arms cannot be told apart: {crossed:?}");

    let queue = home.path().join("land-queue/north");
    std::fs::create_dir_all(queue.join("done")).unwrap();
    std::fs::write(queue.join("done/1-lane.json"), json!({"session": "worker-0-rv", "branch": "lane", "verdict": "landed"}).to_string()).unwrap();
    let lanes = north_base::land_ticket::lanes_by_session(&home.path().join("land-queue")).unwrap();
    let run = |session: &str, overlay: &str| crate::ledger::Run { run_id: 1, session: session.into(), task_ref: None, provider: "opencode".into(), model: None, effort: None, overlay: Some(overlay.into()), account: None, prompt_sha: None, system_prompt_sha: None, deepen: false, started_ms: 1, ended_ms: 2, wall_ms: 1, turns: 1, tokens_in: 0, tokens_out: 0, tokens_cache_read: 0, tokens_cache_write: 0, exit: "completed".into(), tool_calls: None, compactions: None };
    let tallies = super::report::tally(&[run("worker-0", Arm::Overlay.label()), run("worker-1", Arm::Control.label())], &Default::default(), &lanes, None);
    let counts = |arm: Arm| tallies.get(arm.label()).map(|tally| (tally.landed, tally.rejected, tally.pending));
    assert_eq!((counts(Arm::Overlay), counts(Arm::Control)), (Some((1, 0, 0)), Some((0, 0, 1))), "a commit its Sonnet reviewer landed counts on the worker's arm");
}

#[test]
fn a_deepseek_worker_launched_from_a_primary_checkout_starts_in_a_lane_the_launcher_made() {
    let project = tempfile::tempdir().unwrap();
    let main = project.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(project.path().join("worktrees")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let made = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = made.clone();
    let runtime = seeing(Visibility::Private, day("2026-11-10")).with_lanes(Arc::new(move |lane, issue| {
        record.lock().unwrap().push((lane.to_owned(), issue.to_owned()));
        std::fs::create_dir_all(lane).map_err(|error| error.to_string())
    }));

    let spec = runtime.host(&account(home.path()), &request(&main), Opening::Fresh(Some("ses_x")), "").unwrap();

    let lane = project.path().join("worktrees/worker");
    assert_eq!(spec.cwd, lane);
    assert_eq!(*made.lock().unwrap(), [(lane.clone(), "north#1".to_owned())]);
    let resumed = runtime.host(&account(home.path()), &request(&main), Opening::Resume("ses_x"), "").unwrap();
    assert_eq!(resumed.cwd, lane);
    assert_eq!(made.lock().unwrap().len(), 1, "a resumed worker keeps its lane");
}

#[test]
fn an_ended_opencode_host_leaves_its_conversation_but_not_its_tmp_and_cache() {
    let home = tempfile::tempdir().unwrap();
    let scratch = home.path().join("scratch");
    for kept in ["data", "config", "state"] {
        std::fs::create_dir_all(scratch.join(kept)).unwrap();
        std::fs::write(scratch.join(kept).join("session"), "conversation").unwrap();
    }
    for dropped in ["tmp", "cache"] {
        std::fs::create_dir_all(scratch.join(dropped)).unwrap();
        std::fs::write(scratch.join(dropped).join("build"), vec![0u8; 1 << 16]).unwrap();
    }
    let host = turns::Host { harness: &OPENCODE, route: Route::Free, effort: None, session: "ses_x".into(), scratch: scratch.clone(), key_file: None, instructions: None, launcher: Vec::new(), program: fake(home.path()), cwd: home.path().into() };

    host.release_scratch();
    host.release_scratch();

    assert!(["data", "config", "state"].iter().all(|kept| scratch.join(kept).join("session").exists()));
    assert!(["tmp", "cache"].iter().all(|dropped| !scratch.join(dropped).exists()));
}

fn gex45() -> Machine {
    Machine { name: "gex45".into(), ssh: "gex45".into(), agent_ssh: None }
}

#[test]
fn a_session_placed_on_a_build_host_runs_on_the_go_key_that_host_holds() {
    let home = Path::new("/home/tom");
    let account = seeing(Visibility::Private, day("2026-11-10")).account_on(home, Path::new("/none"), "go", &gex45()).unwrap();

    assert_eq!(account.token_file, Some(home.join(".local/state/north/accounts/opencode/go.key")));
    assert!(seeing(Visibility::Private, day("2026-11-10")).account_on(home, Path::new("/none"), "b", &gex45()).is_err());
}
