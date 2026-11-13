use std::path::PathBuf;

use serde_json::json;

use crate::fleet::model::EntryKind;
use crate::fleet::prompt::PromptMode;

use super::*;

const HOOKED: &str = r#"{"hooks": {"PreToolUse": [{}]}}"#;

#[test]
fn a_session_on_an_agent_login_runs_the_published_agent_claude_and_never_a_copy_under_its_home() {
    let machine = Machine { name: "cpx62".into(), ssh: "tom@cpx62".into(), agent_ssh: Some("agent@cpx62".into()) };
    let argv = claude_program(Some(&machine), Path::new("/home/tom"));
    assert_eq!(argv[..2], ["sh", "-c"], "{argv:?}");
    assert!(argv[2].contains("/agent-claude/bin/claude"), "{argv:?}");
    assert!(!argv.iter().any(|word| word.contains(".local/lib") || word.contains("/home/agent")), "{argv:?}");
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("agent-claude/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(&claude, "#!/bin/sh\necho ran\n").unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let missing = dir.path().join("no-such-naming-file");
    let script = agent_runtime::claude_script_naming(&missing.to_string_lossy());
    let output = std::process::Command::new("sh").current_dir(dir.path()).args(["-c", &script, "claude"]).output().unwrap();
    assert_eq!(output.status.code(), Some(127), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let shims = dir.path().join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    std::fs::write(shims.join("stat"), "#!/bin/sh\ncase \"$3\" in %u) echo 0;; %a) echo \"$NAMING_MODE\";; esac\n").unwrap();
    std::fs::set_permissions(shims.join("stat"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let agent_link = dir.path().join("current");
    let naming = dir.path().join("agent-runtime-link");
    std::fs::write(&naming, format!("{}\n", agent_link.display())).unwrap();
    assert_eq!(agent_runtime::claude_link(&agent_link).join("bin/claude"), claude, "the publish lane's link is not the one the agent's argv execs");
    let script = agent_runtime::claude_script_naming(&naming.to_string_lossy());
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap_or_default());
    let run = |mode: &str| std::process::Command::new("sh").env("PATH", &path).env("NAMING_MODE", mode).args(["-c", &script, "claude"]).output().unwrap();
    let ran = run("644");
    assert_eq!((ran.status.code(), String::from_utf8_lossy(&ran.stdout).trim()), (Some(0), "ran"), "{ran:?}");
    for writable in ["2664", "2646", "664", "0646"] {
        let refused = run(writable);
        assert_eq!(refused.status.code(), Some(127), "a root-owned naming file at mode {writable} was followed: {refused:?}");
        assert!(refused.stdout.is_empty(), "{refused:?}");
    }
}

fn request() -> LaunchRequest {
    LaunchRequest {
        verification: false,
        gates_aside: false,
        urgent: false,
        container_shell: false,
        container_dir: None,
        name: "worker".into(),
        provider: Claude.provider(),
        account: "a".into(),
        cwd: PathBuf::from("/tmp"),
        model: None,
        effort: None,
        prompt: String::new(),
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

#[test]
fn a_revived_session_keeps_its_recorded_system_prompt_and_takes_changed_orders_as_one_message() {
    let scratch = tempfile::tempdir().unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: None, api: None };
    let orders = "You are a session in north fleet.\n\n- Think, don't agree.";
    let argv = |opening: Opening| Claude.host(&account, &request(), opening, orders).unwrap().argv;

    let fresh = argv(Opening::Fresh(None));
    assert!(!fresh.iter().any(|arg| arg == "--system-prompt-snapshot"), "{fresh:?}");
    assert!(fresh.windows(2).any(|pair| pair == ["--append-system-prompt", orders]), "{fresh:?}");

    let resumed = argv(Opening::Resume("conversation"));
    assert!(resumed.windows(2).any(|pair| pair == ["--resume", "conversation"]), "{resumed:?}");
    assert!(!resumed.iter().any(|arg| arg == "--system-prompt-snapshot"), "a revive opted out of the recorded prompt: {resumed:?}");

    let opened = |opening: Opening, changed: bool| Claude.open("worker", &request(), opening, Instructions { text: orders, changed }).unwrap();
    let frames = opened(Opening::Resume("conversation"), true);
    assert_eq!(frames.len(), 1, "{frames:?}");
    let frame: serde_json::Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!((frame["type"].as_str(), frame["message"]["role"].as_str(), frame["message"]["content"].as_str()), (Some("user"), Some("user"), Some(orders)));
    assert!(opened(Opening::Resume("conversation"), false).is_empty(), "unchanged orders were delivered again");
    assert!(opened(Opening::Fresh(None), true).is_empty(), "a fresh launch delivers its orders in argv, not as a message");
}

#[test]
fn a_token_account_reads_its_token_into_the_cli_environment_and_never_into_argv_or_the_spec_env() {
    let scratch = tempfile::tempdir().unwrap();
    let token = scratch.path().join("token");
    std::fs::write(&token, "sk-test-value\n").unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: Some(token), api: None };

    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();

    assert!(spec.argv.iter().all(|arg| !arg.contains("sk-test-value")), "{:?}", spec.argv);
    let settings = spec.argv.windows(2).find(|pair| pair[0] == "--settings").map(|pair| serde_json::from_str::<serde_json::Value>(&pair[1]).unwrap());
    assert!(settings.is_some_and(|settings| settings["switchModelsOnFlag"] == true), "a fleet session lets a safety-flagged turn finish on the fallback model: {:?}", spec.argv);
    let interactive = Claude.interactive(&account, &request(), "conversation", "").unwrap();
    assert!(interactive.argv.iter().all(|arg| arg != "--settings"), "Tom's interactive window keeps ~/.claude/settings.json: {:?}", interactive.argv);
    assert!(spec.env.iter().all(|(_, value)| !value.to_string_lossy().contains("sk-test-value")), "{:?}", spec.env);
    assert!(spec.env_remove.contains(&"CLAUDE_CODE_OAUTH_TOKEN".to_owned()), "{:?}", spec.env_remove);
    let probe = [&spec.argv[..4], &["sh".into(), "-c".into(), "printf %s \"$CLAUDE_CODE_OAUTH_TOKEN\"".into()]].concat();
    let output = std::process::Command::new(&probe[0]).args(&probe[1..]).env_remove("CLAUDE_CODE_OAUTH_TOKEN").output().unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "sk-test-value");
    let mut transcript = Transcript::default();
    Claude.observe(
        &json!({
            "type": "system",
            "subtype": "model_refusal_fallback",
            "scope": "session",
            "level": "warning",
            "trigger": "refusal",
            "originalModel": "claude-opus-5",
            "fallbackModel": "claude-opus-4-8",
            "apiRefusalCategory": "cyber",
            "content": "Opus 5's safeguards flagged this message. Switched to Opus 4.8."
        }),
        &mut transcript,
    );
    let rows: Vec<&str> = transcript.entries.iter().filter(|entry| entry.kind == EntryKind::Refusal).map(|entry| entry.text.as_str()).collect();
    assert_eq!(rows, ["safety refusal (cyber: claude-opus-5 -> claude-opus-4-8)"], "{:?}", transcript.entries);
    assert_eq!(Cause::parse("safety refusal"), Some(Cause::Harness(Harness::SafetyRefusal)));
}


#[test]
fn a_brief_on_a_context_arm_launches_claude_with_that_auto_compact_window_and_an_unarmed_brief_keeps_the_settings_window() {
    let scratch = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(scratch.path().join("home")).unwrap();
    std::fs::write(scratch.path().join("home/settings.json"), HOOKED).unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: None, api: None };
    let window = |request: &LaunchRequest| {
        let spec = Claude.host(&account, request, Opening::Fresh(None), "").unwrap();
        spec.env.into_iter().find(|(name, _)| name == "CLAUDE_CODE_AUTO_COMPACT_WINDOW").map(|(_, value)| value.to_string_lossy().into_owned())
    };
    let armed = LaunchRequest { prompt: "Item: north#636\nTier: worker\nArm: context-300k\nGoal: x".into(), ..request() };
    let relaunched = LaunchRequest { compact_window: Some(200_000), ..request() };

    assert_eq!(window(&armed).as_deref(), Some("300000"));
    assert_eq!(window(&relaunched).as_deref(), Some("200000"));
    assert_eq!(window(&LaunchRequest { prompt: "Arm: effort-high".into(), ..request() }), None);
}

#[test]
fn a_profile_without_the_harness_settings_is_named_unguarded_so_its_launch_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("profiles/claude-api");
    std::fs::create_dir_all(&home).unwrap();
    let account = Account { provider: Claude.provider(), label: "api".into(), home: home.clone(), token_file: None, api: None };
    let here = |path: &std::path::Path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text);

    assert!(Claude.unguarded(&account, &here).is_some_and(|reason| reason.contains("no settings.json")));
    std::fs::write(home.join("settings.json"), HOOKED).unwrap();
    assert_eq!(Claude.unguarded(&account, &here), None);
    let worker = LaunchRequest { role: Some(crate::fleet::model::Role::Worker), ..request() };
    std::fs::write(home.join("CLAUDE.md"), "owner policy\n").unwrap();
    let argv = Claude.host(&account, &worker, Opening::Fresh(None), "orders").unwrap().argv;
    let settings: serde_json::Value = serde_json::from_str(&argv[argv.iter().position(|arg| arg == "--settings").unwrap() + 1]).unwrap();
    assert_eq!(settings["hooks"], serde_json::from_str::<serde_json::Value>(HOOKED).unwrap()["hooks"], "a worker's hooks come from the checked settings.json: {argv:?}");
    assert_eq!(argv.iter().filter(|arg| *arg == "--settings").count(), 1, "Claude Code keeps only the last --settings: {argv:?}");
    assert!(argv.windows(2).any(|pair| pair == ["--setting-sources", "project,local"]) && argv.windows(2).any(|pair| pair == ["--system-prompt", "orders\n\nowner policy"]), "a worker's system prompt carries the profile's policy file: {argv:?}");
    let research = LaunchRequest { prompt: "Item: north#1\nCategory: research\n".into(), ..worker };
    let argv = Claude.host(&account, &research, Opening::Fresh(None), "orders").unwrap().argv;
    assert!(argv.windows(2).any(|pair| pair[0] == "--tools" && pair[1].ends_with("WebSearch,WebFetch")), "{argv:?}");

    let remote = scratch.path().join("remote");
    let there = |path: &std::path::Path| std::fs::read_to_string(remote.join(path.strip_prefix("/").unwrap_or(path))).map_or(Seen::Absent, Seen::Text);
    assert!(Claude.unguarded(&account, &there).is_some_and(|reason| reason.contains("no settings.json")));
    let linked = remote.join(home.strip_prefix("/").unwrap());
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(linked.join("settings.json"), HOOKED).unwrap();
    assert_eq!(Claude.unguarded(&account, &there), None);
}

#[test]
fn a_profile_whose_settings_register_no_hooks_is_named_unguarded_too() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("profiles/claude-api");
    std::fs::create_dir_all(&home).unwrap();
    let account = Account { provider: Claude.provider(), label: "api".into(), home: home.clone(), token_file: None, api: None };
    let here = |path: &std::path::Path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text);

    for settings in ["{}", r#"{"hooks": {}}"#, "not json"] {
        std::fs::write(home.join("settings.json"), settings).unwrap();
        assert!(Claude.unguarded(&account, &here).is_some_and(|reason| reason.contains("settings.json that")), "{settings}");
    }
    std::fs::write(home.join("settings.json"), r#"{"hooks": {"PreToolUse": [{}]}}"#).unwrap();
    assert_eq!(Claude.unguarded(&account, &here), None);
    assert_eq!(Claude.unguarded(&account, &|_| Seen::Unreachable), None);
}

#[test]
fn the_script_that_carries_a_conversation_between_account_homes_copies_it_under_its_project_and_leaves_other_conversations_north_1001() {
    let scratch = tempfile::tempdir().unwrap();
    let account = |label: &str| Account { provider: Claude.provider(), label: label.into(), home: scratch.path().join(label), token_file: None, api: None };
    let (from, to) = (account("from"), account("to"));
    std::fs::create_dir_all(from.home.join("projects/-work-lane")).unwrap();
    std::fs::write(from.home.join("projects/-work-lane/kept.jsonl"), "turn\n").unwrap();
    std::fs::write(from.home.join("projects/-work-lane/other.jsonl"), "other\n").unwrap();

    let script = Claude.carry_script(&from, &to, "kept").unwrap();
    assert!(std::process::Command::new("sh").args(["-c", &script]).status().unwrap().success());

    assert_eq!(std::fs::read_to_string(to.home.join("projects/-work-lane/kept.jsonl")).unwrap(), "turn\n");
    assert!(!to.home.join("projects/-work-lane/other.jsonl").exists());
    assert!(Claude.carry_script(&from, &from, "kept").is_none());
}

const PROFILE_ENV: &str = "NORTH_TEST_LAUNCH_PROFILE";
const THERE_ENV: &str = "NORTH_TEST_LAUNCH_THERE";
const STAND_IN_SSH: &str = r#"while [ $# -gt 0 ]; do case "$1" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done
[ -z "$NORTH_TEST_LAUNCH_THERE" ] || printf '%s\n' "$1" >> "$NORTH_TEST_LAUNCH_THERE.logins"
shift
[ "$1" = -- ] && shift
DENY='cat() { [ ! -e "$NORTH_TEST_LAUNCH_THERE.denied" ] || { echo "cat: $2: Permission denied" >&2; return 1; }; command cat "$@"; }'
exec sh -c "$DENY
$(printf '%s' "$*" | sed "s#'/#'$NORTH_TEST_LAUNCH_THERE/#g")"
"#;

fn stand_in_ssh(scratch: &std::path::Path) -> String {
    let bin = scratch.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    north_base::foundation::shim::write_script(&bin.join("ssh"), STAND_IN_SSH);
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())
}

fn placed_worker_reads_its_profile_there(there: &std::path::Path) {
    let home = PathBuf::from("/home/agent/.local/state/north/profiles/claude-e");
    let account = Account { provider: Claude.provider(), label: "e".into(), home: home.clone(), token_file: None, api: None };
    let machine = Machine { name: "cpx62".into(), ssh: "tom@cpx62".into(), agent_ssh: Some("agent@cpx62".into()) };
    let worker = LaunchRequest { role: Some(crate::fleet::model::Role::Worker), machine: Some(machine), ..request() };
    let argv = Claude.host(&account, &worker, Opening::Fresh(None), "orders").unwrap().argv;
    let settings: serde_json::Value = serde_json::from_str(&argv[argv.iter().position(|arg| arg == "--settings").unwrap() + 1]).unwrap();
    assert_eq!(settings["hooks"], serde_json::from_str::<serde_json::Value>(HOOKED).unwrap()["hooks"], "{argv:?}");
    assert!(argv.windows(2).any(|pair| pair == ["--system-prompt", "orders\n\nagent policy"]), "{argv:?}");
    let env: Vec<&String> = settings["env"].as_object().unwrap().keys().collect();
    assert!(env == ["CLAUDE_CODE_SHELL_PREFIX"] && ["apiKeyHelper", "awsCredentialExport", "forceLoginOrgUUID"].iter().all(|key| settings.get(key).is_none()) && settings["theme"] == "dark", "the agent user's settings file chose the worker's credential, endpoint or login: {settings}");
    let logins = std::fs::read_to_string(format!("{}.logins", there.display())).unwrap();
    assert!(!logins.is_empty() && logins.lines().all(|login| login == "agent@cpx62"), "{logins}");
    let refusal = || Claude.host(&account, &worker, Opening::Fresh(None), "orders").err().map(|error| error.to_string()).unwrap_or_default();
    let hooks = || crate::fleet::remote::HooksCheck::start(std::sync::Arc::new(Claude), &account, worker.machine.as_ref()).verdict().unwrap();
    let named = "/home/agent/.local/state/north/profiles/claude-e/settings.json on cpx62 as agent@cpx62";

    std::fs::write(format!("{}.denied", there.display()), "").unwrap();
    let refused = refusal();
    assert!(refused.contains(named) && refused.contains("unreadable there"), "{refused}");
    assert_eq!(hooks(), None, "a file that is there but unreadable is not a profile without settings");
    std::fs::remove_file(format!("{}.denied", there.display())).unwrap();

    std::fs::remove_file(there.join(home.strip_prefix("/").unwrap()).join("settings.json")).unwrap();
    let refused = refusal();
    assert!(refused.contains(named) && refused.contains("no such file"), "{refused}");
    assert!(hooks().is_some_and(|reason| reason.contains("has no settings.json")), "{:?}", hooks());
}

#[test]
fn a_worker_placed_on_another_machine_takes_its_settings_and_policy_from_that_machine_through_its_session_login() {
    if let Some(there) = std::env::var_os(THERE_ENV) {
        return placed_worker_reads_its_profile_there(std::path::Path::new(&there));
    }
    let scratch = tempfile::tempdir().unwrap();
    let north = scratch.path().join("runtime/north");
    std::fs::create_dir_all(scratch.path().join("runtime")).unwrap();
    north_base::foundation::shim::write_script(&north, "exit 0\n");
    let there = scratch.path().join("there");
    let profile = there.join("home/agent/.local/state/north/profiles/claude-e");
    std::fs::create_dir_all(&profile).unwrap();
    let written = json!({"hooks": {"PreToolUse": [{}]}, "theme": "dark", "apiKeyHelper": "cat /home/agent/key", "awsCredentialExport": "cat /home/agent/aws", "forceLoginOrgUUID": "another-org", "env": {"ANTHROPIC_BASE_URL": "https://elsewhere.invalid", "CLAUDE_CODE_OAUTH_TOKEN": "another-account", "CLAUDE_CONFIG_DIR": "/home/agent/elsewhere", "HTTPS_PROXY": "http://proxy.invalid", "NODE_OPTIONS": "--require /home/agent/hook.js"}});
    std::fs::write(profile.join("settings.json"), written.to_string()).unwrap();
    std::fs::write(profile.join("CLAUDE.md"), "agent policy\n").unwrap();

    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["a_worker_placed_on_another_machine_takes_its_settings_and_policy_from_that_machine_through_its_session_login", "--nocapture", "--test-threads=1"])
        .env(THERE_ENV, &there)
        .env("PATH", stand_in_ssh(scratch.path()))
        .env("NORTH_BIN", &north)
        .env_remove("AGENT_CAPACITY_NORTH")
        .output()
        .unwrap();
    assert!(child.status.success(), "{}{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
}
const SHELL_PREFIX: &str = "CLAUDE_CODE_SHELL_PREFIX";

fn the_spawned_tool_server_is_an_existing_executable(profile: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let account = Account { provider: Claude.provider(), label: "a".into(), home: profile.to_path_buf(), token_file: None, api: None };
    let placed = crate::fleet::model::Machine { name: "far".into(), ssh: "tom@far".into(), agent_ssh: None };
    for machine in [None, Some(placed)] {
        let worker = LaunchRequest { role: Some(crate::fleet::model::Role::Worker), machine, ..request() };
        let spec = Claude.host(&account, &worker, Opening::Fresh(None), "orders").unwrap();
        let last = |flag: &str| spec.argv.windows(2).rev().find(|pair| pair[0] == flag).map(|pair| pair[1].clone());
        let config: serde_json::Value = serde_json::from_str(&last("--mcp-config").expect("a worker launch configures its tool server")).unwrap();
        let entry = &config["mcpServers"]["north"];
        assert!(entry["args"].as_array().is_some_and(|args| args.iter().all(serde_json::Value::is_string)), "the entry's arguments are a list: {entry}");
        let settings = last("--settings").unwrap();
        let settings: serde_json::Value = serde_json::from_str(&settings).unwrap_or_else(|_| serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap());
        let process = spec.env.iter().find(|(name, _)| name == SHELL_PREFIX).map(|(_, value)| value.to_string_lossy().into_owned());
        let prefix = settings["env"][SHELL_PREFIX].as_str().map(str::to_owned).or(process);
        let command = entry["command"].as_str().unwrap();
        for program in [prefix.as_deref().unwrap_or(command), command] {
            let executable = std::fs::metadata(program).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
            assert!(executable, "Claude Code spawns {program:?} as one program path for the tool server, and it is no executable: {:?}", spec.argv);
        }
    }
}

#[test]
fn a_worker_launch_writes_a_tool_server_entry_whose_command_is_an_existing_executable() {
    if let Some(profile) = std::env::var_os(PROFILE_ENV) {
        return the_spawned_tool_server_is_an_existing_executable(std::path::Path::new(&profile));
    }
    let scratch = tempfile::tempdir().unwrap();
    let north = scratch.path().join("runtime/north");
    std::fs::create_dir_all(scratch.path().join("runtime")).unwrap();
    north_base::foundation::shim::write_script(&north, "exit 0\n");
    let profile = scratch.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    let projected = json!({"hooks": {"PreToolUse": [{}]}, "env": {SHELL_PREFIX: format!("{} --worker-sandbox", north.display())}});
    std::fs::write(profile.join("settings.json"), projected.to_string()).unwrap();

    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["a_worker_launch_writes_a_tool_server_entry_whose_command_is_an_existing_executable", "--nocapture", "--test-threads=1"])
        .env(PROFILE_ENV, &profile)
        .env("PATH", stand_in_ssh(scratch.path()))
        .env("NORTH_BIN", &north)
        .env_remove("AGENT_CAPACITY_NORTH")
        .output()
        .unwrap();
    assert!(child.status.success(), "{}{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
}
