use std::io::Cursor;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use landlock::RulesetStatus;
use north_providers::fleet::model::{Account, LaunchRequest};
use north_providers::fleet::prompt::PromptMode;
use north_providers::fleet::runtime::{Opening, Runtime};
use north_providers::providers::opencode::harness::OPENCODE;
use north_providers::providers::opencode::{Deepseek, Route, Visibility, route, turns};
use north_runtime::worker_sandbox::{Context, confine_to_lane};
use serde_json::Value;

const LAUNCHER: &str = "lane_only_launcher";

#[test]
fn lane_only_launcher() {
    let arguments: Vec<String> = std::env::args().collect();
    let Some(at) = arguments.iter().position(|word| word == "--") else {
        return;
    };
    let (home, program) = (&arguments[at + 1], &arguments[at + 2..]);
    let mut environment: Vec<(String, String)> = std::env::vars().filter(|(name, _)| name != "HOME").collect();
    environment.push(("HOME".into(), home.clone()));
    let context = Context::from_request("", &std::env::current_dir().unwrap(), &environment);
    assert!(matches!(confine_to_lane(&context, &[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let error = std::process::Command::new(&program[0]).args(&program[1..]).exec();
    panic!("{}: {error}", program[0]);
}

fn request(cwd: &Path, prompt: &str) -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: "worker".into(),
        provider: Deepseek::opencode().provider(),
        account: "go".into(),
        cwd: cwd.into(),
        model: None,
        effort: None,
        prompt: prompt.into(),
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
        urgent: false,
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn an_opencode_worker_in_a_public_lane_cannot_read_south_or_sops_keys() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let root = tempfile::Builder::new().prefix("north-1020-routed-lane").tempdir_in(outside_tmp).unwrap();
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    let lane = home.join("code/north/worktrees/lane");
    let own = lane.join("brief.md");
    let secret = [
        home.join("code/south/main/README.md"),
        home.join(".config/sops/age/keys.txt"),
        home.join(".local/state/north/fleet/sessions/proxy.json"),
        home.join(".local/state/agents/handoffs/proxy.md"),
        home.join(".claude/.credentials.json"),
        home.join(".local/share/opencode/auth.json"),
        home.join("code/wisp/main/README.md"),
    ];
    write(&home.join("code/north/main/.git/HEAD"), "ref: refs/heads/main\n");
    write(&own, "the lane's own file\n");
    for file in &secret {
        write(file, "AGE-SECRET-KEY-1QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ\n");
    }

    let runtime = Deepseek::reading(&OPENCODE, Arc::new(|_| Visibility::Public), Arc::new(|| route::date("2026-11-10").unwrap()));
    let account = Account { provider: Deepseek::opencode().provider(), label: "go".into(), home: root.join("account"), token_file: None, api: None };
    let launch = |prompt: &str| runtime.host(&account, &request(&lane, prompt), Opening::Fresh(Some("ses_x")), "");
    let refused = launch("Item: north#1\nBox: copy ~/code/south/main/README.md, as tompassarelli/south#94 asks").err().map(|error| error.user_message()).unwrap_or_default();
    assert!(refused.contains("names a private repository") && refused.contains("free model may train"), "a brief naming a private repository reached the free model: {refused}");
    let spec = launch("Item: north#1\nBox: read the lane").unwrap();
    let picked = spec.argv.iter().position(|word| word == "--route").and_then(|at| spec.argv.get(at + 1)).cloned().unwrap_or_default();
    assert_eq!(Route::parse(&picked), Some(Route::Free), "{:?}", spec.argv);

    let probes: Vec<String> = std::iter::once(&own).chain(&secret).map(|path| path.display().to_string()).collect();
    let program = root.join("bin/opencode");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    let body = format!(
        "seen=''\nfor path in {}; do if cat \"$path\" >/dev/null 2>&1; then seen=\"$seen read:$path\"; else seen=\"$seen refused:$path\"; fi; done\nprintf '{{\"type\":\"text\",\"sessionID\":\"ses_x\",\"part\":{{\"text\":\"Done:%s\"}}}}\\n' \"$seen\"\nprintf '%s\\n' '{{\"type\":\"step_finish\",\"sessionID\":\"ses_x\",\"part\":{{\"reason\":\"stop\",\"tokens\":{{\"input\":1,\"output\":1,\"reasoning\":0,\"cache\":{{\"read\":0,\"write\":0}}}}}}}}'\n",
        probes.join(" ")
    );
    north_providers::foundation::shim::write_script(&program, &body);
    let launcher = [std::env::current_exe().unwrap().display().to_string(), "--exact".into(), LAUNCHER.into(), "--nocapture".into(), "--quiet".into(), "--".into(), home.display().to_string()].into();
    let host = turns::Host { harness: &OPENCODE, route: Route::Free, effort: None, session: "ses_x".into(), scratch: root.join("scratch"), key_file: None, instructions: None, launcher, program, cwd: lane };
    let mut output = Vec::new();
    turns::serve(&host, Cursor::new("{\"type\":\"user\",\"uuid\":\"u0\",\"text\":\"Box: read the lane\"}\n"), &mut output).unwrap();

    let frames: Vec<Value> = String::from_utf8(output).unwrap().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let report = frames.iter().find(|frame| frame["type"] == "text").and_then(|frame| frame["part"]["text"].as_str()).unwrap_or_else(|| panic!("the worker reported nothing: {frames:?}"));
    assert!(report.contains(&format!("read:{}", own.display())), "the worker could not read its own lane: {report}");
    let read: Vec<&PathBuf> = secret.iter().filter(|path| !report.contains(&format!("refused:{}", path.display()))).collect();
    assert!(read.is_empty(), "an opencode worker in a public lane read {read:?}: {report}");
}
