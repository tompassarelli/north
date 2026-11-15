use crate::capacity_host::Host;
use serde_json::{Value, json};
use std::fs;

const FIRST: &str = "first.signed-in@example.test";
const SECOND: &str = "second.signed-in@example.test";

fn hook_refusal(host: &Host, private: bool, body: &str) -> Option<String> {
    let request = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": format!("gh issue comment 5 --body '{body}'") },
        "cwd": host.path("").display().to_string(),
    });
    fs::write(host.path("request.json"), request.to_string()).unwrap();
    let output = host
        .command(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["hook", "pre-bash"])
        .stdin(fs::File::open(host.path("request.json")).unwrap())
        .env("HOME", host.path("home"))
        .env("AGENTS_ORCHESTRATION", host.path("orchestration.toml"))
        .env("RESIDENT_CAP_HOST", "laptop")
        .env("NORTH_PUBLIC_DENYLIST", host.path("denylist.toml"))
        .env("FAKE_PRIVATE", if private { "true" } else { "false" })
        .output()
        .unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let decision = &answer["hookSpecificOutput"];
    (decision["permissionDecision"] == "deny").then(|| decision["permissionDecisionReason"].as_str().unwrap_or_default().to_owned())
}

#[test]
fn public_body_with_an_account_email_is_refused_and_a_private_repository_takes_it() {
    let host = Host::new("public-account-email-", &[("gh", "#!/bin/sh\n: north gh-session stand-in for the repository visibility read\necho \"${FAKE_PRIVATE:-false}\"\n")]);
    fs::create_dir_all(host.path("home")).unwrap();
    fs::write(host.path("home/.claude.json"), json!({ "oauthAccount": { "emailAddress": FIRST } }).to_string()).unwrap();
    fs::create_dir_all(host.path("profiles/second")).unwrap();
    fs::write(host.path("profiles/second/.claude.json"), json!({ "oauthAccount": { "emailAddress": SECOND } }).to_string()).unwrap();
    fs::write(host.path("orchestration.toml"), format!("[accounts.claude.profiles.second.laptop]\npath = \"{}\"\n", host.path("profiles/second").display())).unwrap();
    fs::write(host.path("denylist.toml"), "").unwrap();

    for email in [FIRST, SECOND] {
        let body = format!("the handoff is signed in as {email}");
        let refusal = hook_refusal(&host, false, &body).unwrap_or_else(|| panic!("a public body naming {email} passed"));
        assert!(refusal.contains("names a signed-in account by its sign-in email"), "{refusal}");
        assert!(!refusal.contains(email), "{refusal}");
        assert!(hook_refusal(&host, true, &body).is_none(), "a private repository refused {email}");
    }
}
