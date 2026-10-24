mod guard {
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    use std::process::{Child, Command, Output, Stdio};
    use std::time::{Duration, Instant};

    const DENIAL: &str = "No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.";
    const SPEC: &str = "niri msg action focus-window --id 22 && niri msg action fullscreen-window";
    const READ: &str = "niri msg -j windows";

    struct Fixture {
        root: tempfile::TempDir,
        env: BTreeMap<String, String>,
        child: Option<Child>,
        _socket: UnixListener,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("desktop-action-")
                .tempdir()
                .unwrap();
            let p = root.path();
            std::fs::copy(
                std::env::var_os("NORTH_DESKTOP_BINARY")
                    .unwrap_or_else(|| env!("CARGO_BIN_EXE_north").into()),
                p.join("north"),
            )
            .unwrap();
            for dir in ["runtime", "bin", "run-user/private-desktop.fixture/runtime"] {
                std::fs::create_dir_all(p.join(dir)).unwrap();
            }
            std::fs::write(
                p.join("gen.active"),
                "hook north-hook\n",
            )
            .unwrap();
            std::fs::write(p.join("gen.json"), json!({"schema":"north.agent-activation/v1", "catalogDigest":format!("sha256:{}", "0".repeat(64)), "generationId":format!("sha256:{}", "1".repeat(64)), "units":[{"kind":"hook", "id":"north-hook", "permission":"on", "active":true}]}).to_string()).unwrap();
            let niri = p.join("bin/niri");
            std::fs::write(&niri, "#!/usr/bin/env bash\nprintf \"%s\\n\" \"$*\" >> \"$NIRI_LOG\"\ncase \"$*\" in \"msg -j focused-window\") printf \"%s\\n\" \"${NIRI_FOCUSED:-null}\";;\n\"msg -j windows\") [ -n \"${NIRI_WINDOWS:-}\" ] || exit 1; printf \"%s\\n\" \"$NIRI_WINDOWS\";;\n\"msg action focus-window --id 7\") [ -z \"${NIRI_CLOSED:-}\" ] || exit 1;; esac\n").unwrap();
            std::fs::set_permissions(&niri, std::fs::Permissions::from_mode(0o755)).unwrap();
            let private = p.join("run-user/private-desktop.fixture");
            for marker in ["active", "lifecycle-owned"] {
                std::fs::write(private.join(marker), "").unwrap();
            }
            std::fs::write(private.join("wayland-display"), "wayland-9\n").unwrap();
            let socket = UnixListener::bind(private.join("runtime/wayland-9")).unwrap();
            let mut env: BTreeMap<String, String> = std::env::vars().collect();
            for (key, value) in [
                ("NORTH_AGENT_ACTIVE", p.join("gen.active").display().to_string()),
                ("NORTH_AGENT_ACTIVATION", p.join("gen.json").display().to_string()),
                ("AGENT_NO_AUTHORING_HOOKS", "0".into()),
                ("XDG_STATE_HOME", p.join("state").display().to_string()),
                ("XDG_RUNTIME_DIR", p.join("runtime").display().to_string()),
                ("WAYLAND_DISPLAY", "wayland-1".into()),
                ("NORTH_PRIVATE_DESKTOP_ROOT", p.join("run-user").display().to_string()),
                ("AGENT_HOOK_ERRORS", p.join("errors.tsv").display().to_string()),
                ("PATH", format!("{}:{}", p.join("bin").display(), std::env::var("PATH").unwrap())),
                ("NIRI_LOG", p.join("niri.log").display().to_string()),
                ("NIRI_WINDOWS", json!([{"id":3,"app_id":"quiet.spawn","is_focused":false},{"id":4,"app_id":"quiet.spawn","is_focused":true},{"id":5,"app_id":"com.mitchellh.ghostty","is_focused":false}]).to_string()),
            ] { env.insert(key.into(), value); }
            for key in ["WAYLAND_SOCKET", "NORTH_HOOK_ID"] {
                env.remove(key);
            }
            let bash = Command::new("bash")
                .args(["-c", "command -v bash"])
                .output()
                .unwrap();
            let fake = p.join("swayidle");
            std::fs::copy(String::from_utf8(bash.stdout).unwrap().trim(), &fake).unwrap();
            let child = Command::new(fake)
                .args([
                    "-c",
                    "\"$0\" desktop-idle-state idle; read -r _",
                    p.join("north").to_str().unwrap(),
                ])
                .env_clear()
                .envs(&env)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            Self {
                root,
                env,
                child: Some(child),
                _socket: socket,
            }
        }

        fn state(&self, value: &Value) {
            std::fs::write(
                self.root.path().join("runtime/desktop-idle.json"),
                value.to_string(),
            )
            .unwrap();
        }

        fn invoke(
            &self,
            provider: &str,
            command: &str,
            body: Option<&str>,
            overrides: &[(&str, &str)],
        ) -> (Value, Option<Value>) {
            let field = if provider == "exec_command" || provider == "functions.exec_command" {
                "cmd"
            } else {
                "command"
            };
            let input = if field == "cmd" {
                json!({"cmd":command,"workdir":self.root.path(),"yield_time_ms":1000,"max_output_tokens":1000})
            } else {
                json!({"command":command,"timeout":1000,"description":"Desktop step","run_in_background":false})
            };
            let mut payload = json!({"hook_event_name":"PreToolUse","tool_name":if field == "cmd" {provider} else {"Bash"},"cwd":std::env::var("HOME").unwrap(),"session_id":format!("{provider}-fixture"),"tool_input":if field == "cmd" {json!(input.to_string())} else {input.clone()},"tool_use_id":"call_fixture"});
            if provider != "claude" {
                payload["turn_id"] = json!("fixture-turn");
                payload["permission_mode"] = json!("bypassPermissions");
            }
            let text = payload.to_string();
            let mut env = self.env.clone();
            for (k, v) in overrides {
                env.insert(k.to_string(), v.to_string());
                if *k == "NORTH_AGENT_ACTIVE" {
                    env.remove("NORTH_AGENT_ACTIVATION");
                }
            }
            let output = run(
                self.root.path().join("north").to_str().unwrap(),
                &["hook"],
                body.unwrap_or(&text),
                &env,
            );
            assert!(output.status.success(), "guard exit: {:?}", output);
            let decoded = decode(&output);
            (input, decoded.map(|v| v["hookSpecificOutput"].clone()))
        }

        fn execute(&self, text: &str, focused: &str, closed: &str) -> (Output, Vec<String>) {
            std::fs::write(self.root.path().join("niri.log"), "").unwrap();
            let output = Command::new("bash")
                .args(["-c", text])
                .env_clear()
                .envs(&self.env)
                .env("NIRI_FOCUSED", focused)
                .env("NIRI_CLOSED", closed)
                .output()
                .unwrap();
            let calls = std::fs::read_to_string(self.root.path().join("niri.log"))
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect();
            (output, calls)
        }
    }

    fn run(program: &str, args: &[&str], text: &str, env: &BTreeMap<String, String>) -> Output {
        let mut child = Command::new(program)
            .args(args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(text.as_bytes()) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }

    fn decode(output: &Output) -> Option<Value> {
        if output.stdout.is_empty() {
            None
        } else {
            Some(serde_json::from_slice(&output.stdout).unwrap())
        }
    }

    #[test]
    fn desktop_action_guard() {
        let policy: toml_edit::DocumentMut = include_str!("../harness/policy-owners.toml")
            .parse()
            .unwrap();
        let owner = policy["guard"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row.as_inline_table()
                    .unwrap()
                    .get("unit")
                    .and_then(toml_edit::Value::as_str)
                    == Some("north-hook")
            })
            .unwrap();
        for provider in ["Bash", "exec_command", "functions.exec_command"] {
            assert!(
                owner
                    .as_inline_table()
                    .unwrap()
                    .get("codex")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|binding| {
                        let (event, matcher) = binding.as_str().unwrap().split_once(':').unwrap();
                        event == "PreToolUse"
                            && regex::Regex::new(matcher).unwrap().is_match(provider)
                    }),
                "real tool routed: {provider}"
            );
        }
        let mut f = Fixture::new();
        let state_path = f.root.path().join("runtime/desktop-idle.json");
        let start = Instant::now();
        let idle: Value = loop {
            if let Ok(bytes) = std::fs::read(&state_path)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                    && value["idle"] == true {
                        break value;
                    }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "swayidle callback did not record state"
            );
            std::thread::yield_now();
        };
        let mut busy = idle.clone();
        busy["idle"] = json!(false);
        assert_eq!(
            Command::new(f.root.path().join("north"))
                .args(["desktop-idle-state", "idle"])
                .env_clear()
                .envs(&f.env)
                .status()
                .unwrap()
                .code(),
            Some(1)
        );
        let private = f
            .root
            .path()
            .join("run-user/private-desktop.fixture")
            .display()
            .to_string();
        let denied = vec![
            SPEC.to_owned(),
            "wtype hello".into(),
            "ydotool key 28:1".into(),
            "niri msg action close-window".into(),
            "niri msg action move-column-left".into(),
            "echo \"$(niri msg action focus-window --id 22)\"".into(),
            "bash -c \"niri msg action fullscreen-window\"".into(),
            "eval \"wtype hi\"".into(),
            "nice wtype hi".into(),
            "stdbuf -o0 wtype hi".into(),
            "xargs wtype <<< hi".into(),
            "spawn-quiet niri msg action focus-window --id 3".into(),
            "spawn-quiet wtype x".into(),
            "niri msg action close-window --id 4".into(),
            "niri msg action close-window --id 5".into(),
            "niri msg action close-window --id 9".into(),
            "niri msg action focus-window --id 3".into(),
            format!("XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-1 wtype hi"),
            format!("XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 ydotool type hi"),
        ];
        let allowed = vec![
            READ.to_owned(),
            "niri msg -j focused-window".into(),
            "echo \"niri msg action focus-window --id 22\"".into(),
            "cat <<'EOF'\nwtype hi\nEOF".into(),
            "rg wtype src".into(),
            "spawn-quiet some-app".into(),
            "niri msg action close-window --id 3".into(),
            "niri msg action close-window --id=3".into(),
            "niri msg action close-window --id 3 --id".into(),
            "spawn-quiet ghostty --title=x -e claude \"brief\"".into(),
            "automation-chrome status".into(),
            "bash harness/skills/private-desktop-development/scripts/private-desktop.sh".into(),
            format!("XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 wtype hello"),
            format!(
                "env XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 wtype -k Return"
            ),
        ];
        let mut payloads = 0;
        f.state(&busy);
        let multibyte = json!({"hook_event_name":"PreToolUse", "tool_name":"Bash", "tool_input":{"command":format!("wtype {}", "é".repeat(600_000))}}).to_string();
        assert_eq!(
            f.invoke("claude", "", Some(&multibyte), &[]).1.unwrap()["permissionDecision"],
            "deny"
        );
        payloads += 1;
        let deep = format!(
            "{{\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":\"wtype hi\",\"opaque\":{}0{}}}}}",
            "[".repeat(130),
            "]".repeat(130)
        );
        assert_eq!(
            f.invoke("claude", "", Some(&deep), &[]).1.unwrap()["permissionDecision"],
            "deny"
        );
        payloads += 1;
        for state in [&busy, &idle] {
            f.state(state);
            for provider in ["Bash", "exec_command", "functions.exec_command"] {
                for as_string in [false, true] {
                    for opaque in [r#""\ud800""#, "18446744073709551617"] {
                        let field = if provider == "Bash" { "command" } else { "cmd" };
                        let input = format!("{{\"{field}\":\"wtype hi\",\"opaque\":{opaque}}}");
                        let input = if as_string {
                            json!(input).to_string()
                        } else {
                            input
                        };
                        let payload = format!(
                            "{{\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"{provider}\",\"tool_input\":{input}}}"
                        );
                        let output = run(
                            f.root.path().join("north").to_str().unwrap(),
                            &["hook"],
                            &payload,
                            &f.env,
                        );
                        assert!(output.status.success());
                        let outer: BTreeMap<String, Box<serde_json::value::RawValue>> =
                            serde_json::from_slice(&output.stdout).unwrap();
                        let hook: BTreeMap<String, Box<serde_json::value::RawValue>> =
                            serde_json::from_str(outer["hookSpecificOutput"].get()).unwrap();
                        if state["idle"] == true {
                            assert_eq!(hook["permissionDecision"].get(), r#""allow""#);
                            let updated: BTreeMap<String, Box<serde_json::value::RawValue>> =
                                serde_json::from_str(hook["updatedInput"].get()).unwrap();
                            assert_eq!(updated["opaque"].get(), opaque);
                        } else {
                            assert_eq!(hook["permissionDecision"].get(), r#""deny""#);
                            assert_eq!(
                                hook["permissionDecisionReason"].get(),
                                json!(DENIAL).to_string()
                            );
                            assert_eq!(output.stdout.len(), 260);
                        }
                        payloads += 1;
                    }
                }
            }
        }
        for provider in ["claude", "codex", "exec_command", "functions.exec_command"] {
            let field = if provider.contains("exec_command") {
                "cmd"
            } else {
                "command"
            };
            f.state(&busy);
            for command in &denied {
                assert_eq!(
                    f.invoke(provider, command, None, &[]).1,
                    Some(
                        json!({"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":DENIAL})
                    ),
                    "{provider}: {command}"
                );
                payloads += 1;
            }
            assert_eq!(
                f.invoke(
                    provider,
                    "niri msg action close-window --id 3",
                    None,
                    &[("NIRI_WINDOWS", "")]
                )
                .1
                .unwrap()["permissionDecision"],
                "deny"
            );
            payloads += 1;
            for command in &allowed {
                assert_eq!(
                    f.invoke(provider, command, None, &[]).1,
                    None,
                    "{provider}: {command}"
                );
                payloads += 1;
            }
            std::fs::remove_file(&state_path).unwrap();
            assert_eq!(
                f.invoke(provider, SPEC, None, &[]).1.unwrap()["permissionDecision"],
                "deny"
            );
            payloads += 1;
            f.state(&idle);
            let (input, output) = f.invoke(provider, SPEC, None, &[]);
            payloads += 1;
            let output = output.unwrap();
            assert_eq!(output["permissionDecision"], "allow");
            assert!(
                output["additionalContext"]
                    .as_str()
                    .unwrap()
                    .contains("desktop action guard")
            );
            let mut rewritten = output["updatedInput"].clone();
            let wrapped = rewritten[field].as_str().unwrap().to_owned();
            rewritten[field] = input[field].clone();
            assert_eq!(rewritten, input);
            assert_eq!(f.invoke(provider, READ, None, &[]).1, None);
            payloads += 1;
            let missing = f.root.path().join("missing.active").display().to_string();
            let oversized = "x".repeat(1_100_000);
            for (body, overrides) in [
                (None, vec![("AGENT_NO_AUTHORING_HOOKS", "1")]),
                (None, vec![("NORTH_AGENT_ACTIVE", missing.as_str())]),
                (Some("{\"tool_name\":"), vec![]),
                (Some(oversized.as_str()), vec![]),
            ] {
                f.state(&busy);
                assert_eq!(f.invoke(provider, SPEC, body, &overrides).1, None);
                payloads += 1;
            }
            for closed in ["", "1"] {
                let (result, calls) = f.execute(&wrapped, "{\"id\":7}", closed);
                assert_eq!(result.status.code(), Some(0));
                assert_eq!(
                    calls,
                    [
                        "msg -j focused-window",
                        "msg action focus-window --id 22",
                        "msg action fullscreen-window",
                        "msg action focus-window --id 7"
                    ]
                );
            }
            f.state(&idle);
            let failing = f
                .invoke(
                    provider,
                    "niri msg action focus-window --id 22; exit 3",
                    None,
                    &[],
                )
                .1
                .unwrap()["updatedInput"][field]
                .as_str()
                .unwrap()
                .to_owned();
            payloads += 1;
            let (result, calls) = f.execute(&failing, "{\"id\":7}", "");
            assert_eq!(result.status.code(), Some(3));
            assert_eq!(calls.last().unwrap(), "msg action focus-window --id 7");
            let (result, calls) = f.execute(&failing, "null", "");
            assert_eq!(result.status.code(), Some(3));
            assert_eq!(
                calls,
                ["msg -j focused-window", "msg action focus-window --id 22"]
            );
        }
        f.child.as_mut().unwrap().kill().unwrap();
        f.child.as_mut().unwrap().wait().unwrap();
        f.state(&idle);
        assert_eq!(
            f.invoke("claude", SPEC, None, &[]).1.unwrap()["permissionDecision"],
            "deny"
        );
        payloads += 1;
        let history = f.root.path().join("state/agents/presence/idle.jsonl");
        let before = std::fs::read_to_string(&history).unwrap().lines().count();
        assert!(
            Command::new(f.root.path().join("swayidle"))
                .args([
                    "-c",
                    "\"$0\" desktop-idle-state active; status=$?; exit \"$status\"",
                    f.root.path().join("north").to_str().unwrap()
                ])
                .env_clear()
                .envs(&f.env)
                .status()
                .unwrap()
                .success()
        );
        let rows: Vec<Value> = std::fs::read_to_string(&history)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(rows.len(), before + 1);
        assert_eq!(rows.last().unwrap()["idle"], false);
        assert!(!rows.last().unwrap()["at"].as_str().unwrap().is_empty());
        eprintln!(
            "desktop action guard: {payloads} fixture payloads; all original behavioral assertions passed"
        );
    }
}
