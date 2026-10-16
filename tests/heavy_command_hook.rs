use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn heavy_command_policy_routes_both_provider_payloads_only_when_active() {
    let scratch = tempfile::tempdir().unwrap();
    let activation = scratch.path().join("activation.json");
    let unit = json!({"id":"heavy-command-guard", "kind":"hook", "permission":"on", "active":true});
    let valid = json!({"schema":"north.agent-activation/v1", "catalogDigest":format!("sha256:{}", "a".repeat(64)), "generationId":format!("sha256:{}", "b".repeat(64)), "units":[unit.clone()]});
    let mut inactive = valid.clone();
    inactive["units"][0]["active"] = json!(false);
    let mut duplicate = valid.clone();
    duplicate["units"].as_array_mut().unwrap().push(unit);
    for (generation, disabled, expected) in [
        (valid.clone(), false, true),
        (inactive, false, false),
        (duplicate, false, false),
        (json!({}), false, false),
        (valid, true, false),
    ] {
        std::fs::write(&activation, serde_json::to_vec(&generation).unwrap()).unwrap();
        for (tool, field, array) in [
            ("Bash", "command", false),
            ("functions.exec_command", "cmd", false),
            ("local_shell", "command", true),
        ] {
            let mut original =
                json!({"workdir":scratch.path(), "opaque":{"preserve":true}, "timeout":1000});
            original[field] = if array {
                json!(["bash", "-lc", "bun test a.test.ts b.test.ts"])
            } else {
                json!("bun test a.test.ts b.test.ts")
            };
            let tool_input = if tool == "functions.exec_command" {
                json!(original.to_string())
            } else {
                original.clone()
            };
            let payload = json!({"tool_name":tool, "cwd":scratch.path(), "tool_input":tool_input});
            let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
                .args(["hook", "pre-bash"])
                .env_remove("NORTH_HOOK_ID")
                .env("NORTH_AGENT_ACTIVATION", &activation)
                .env("NORTH_AGENT_ACTIVE", scratch.path().join("missing.active"))
                .env("AGENT_NO_AUTHORING_HOOKS", if disabled { "1" } else { "" })
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(payload.to_string().as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if expected {
                let response: Value = serde_json::from_slice(&output.stdout).unwrap();
                let updated = &response["hookSpecificOutput"]["updatedInput"];
                assert_eq!(
                    response["hookSpecificOutput"]["permissionDecision"],
                    "allow"
                );
                let command = if array {
                    &updated[field][2]
                } else {
                    &updated[field]
                };
                assert!(command.as_str().unwrap().contains("machine-capacity.mjs"));
                for key in ["opaque", "timeout", "workdir"] {
                    assert_eq!(updated[key], original[key]);
                }
            } else {
                assert!(
                    output.stdout.is_empty(),
                    "{tool}: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
        }
    }
}
