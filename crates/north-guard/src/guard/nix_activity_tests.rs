use super::*;
use serde_json::json;

fn call(tool: &str, session: &str, input: serde_json::Value) -> HookInput {
    let payload = json!({
        "tool_name": tool,
        "session_id": session,
        "cwd": "/home/tom/code/north/worktrees/nix-d3",
        "tool_input": input,
    });
    HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
}

fn bash(session: &str, command: &str) -> HookInput {
    call("Bash", session, json!({ "command": command }))
}

fn classes(text: &str) -> Vec<String> {
    text.lines().filter_map(|line| line.strip_prefix("nix-activity: ")).filter_map(|rest| rest.split_whitespace().next()).filter(|word| word.starts_with('N') && word[1..].chars().all(|c| c.is_ascii_digit())).map(str::to_owned).collect()
}

fn observed(command: &str) -> String {
    let seen = tempfile::tempdir().unwrap();
    match decide(&bash("fixture", command), seen.path()) {
        Verdict::Deny(reason) => format!("deny {}", classes(&reason).join(" ")),
        Verdict::Advise(text) => classes(&text).join(" "),
        _ => String::new(),
    }
}

#[test]
fn recorded_commands_match_their_class() {
    let cases: Vec<(String, String)> = serde_json::from_str(include_str!("fixtures/nix_activity.json")).unwrap();
    assert!(cases.len() >= 20);
    for (command, expected) in cases {
        assert_eq!(observed(&command), expected, "{command}");
    }
}

#[test]
fn commands_that_only_mention_nix_match_nothing() {
    for command in ["git commit -m 'nix develop breaks land-shell'", "rg cargo src", "echo nix develop -c cargo test", "ls worktrees", "cat <<'EOF'\nnix-collect-garbage\nEOF"] {
        assert_eq!(observed(command), "", "{command}");
    }
}

#[test]
fn a_wrapped_land_check_is_denied_with_the_n1_card() {
    let seen = tempfile::tempdir().unwrap();
    let command = "./bin/land-shell --dev -- target/debug/north land checks --base origin/main --head HEAD";
    let Verdict::Deny(reason) = decide(&bash("replay", command), seen.path()) else {
        panic!("not denied");
    };
    let n1 = cards().into_iter().find(|card| card.class == "N1").unwrap();
    assert!(reason.contains(&n1.render()), "{reason}");
    assert!(reason.contains("every check exits 127"), "{reason}");
}

#[test]
fn a_card_is_injected_once_per_session_per_class() {
    let seen = tempfile::tempdir().unwrap();
    let Verdict::Advise(text) = decide(&bash("one", "nix develop -c cargo test"), seen.path()) else {
        panic!("no cards");
    };
    assert_eq!(classes(&text), ["N1", "N2"]);
    assert_eq!(decide(&bash("one", "nix develop -c cargo test"), seen.path()), Verdict::Allow);
    assert!(matches!(decide(&bash("two", "nix develop -c cargo test"), seen.path()), Verdict::Advise(_)));
    let Verdict::Advise(later) = decide(&bash("one", "nix build .#north"), seen.path()) else {
        panic!("new class not injected");
    };
    assert_eq!(classes(&later), ["N4"]);
}

#[test]
fn a_denied_form_is_denied_on_every_call() {
    let seen = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        assert!(matches!(decide(&bash("gc", "nix-collect-garbage -d"), seen.path()), Verdict::Deny(_)));
    }
}

#[test]
fn edits_to_nix_sources_get_their_card() {
    let seen = tempfile::tempdir().unwrap();
    let cases = [("flake.lock", "N9"), ("flake.nix", "N10"), (".envrc", "N6"), ("tests/x.test.sh", "N8")];
    for (path, class) in cases {
        let input = call("Edit", "edit", json!({ "file_path": format!("/home/tom/code/north/worktrees/nix-d3/{path}"), "new_string": "x" }));
        let Verdict::Advise(text) = decide(&input, seen.path()) else {
            panic!("{path}");
        };
        assert_eq!(classes(&text), [class], "{path}");
    }
    let script = call("Write", "edit", json!({ "file_path": "/home/tom/code/north/worktrees/nix-d3/bin/new-tool", "content": "exec nix develop -c true" }));
    assert!(matches!(decide(&script, seen.path()), Verdict::Advise(_)));
}

#[test]
fn every_class_has_one_short_card() {
    let all = cards();
    let names: Vec<&str> = all.iter().map(|card| card.class.as_str()).collect();
    let expected: Vec<String> = (1..=12).map(|n| format!("N{n}")).collect();
    assert_eq!(names, expected);
    for card in &all {
        assert!(card.render().lines().count() <= 10, "{}", card.class);
        assert!(!card.incident.is_empty() && !card.title.is_empty());
    }
}

#[test]
fn nix_gc_is_named_by_the_deny_of_every_store_collection_form() {
    let seen = tempfile::tempdir().unwrap();
    for command in ["run-bounded 20m -- nix-collect-garbage", "nix-store --gc", "nix store gc", "sudo nix-collect-garbage -d"] {
        let Verdict::Deny(reason) = decide(&bash("6430b666", command), seen.path()) else {
            panic!("not denied: {command}");
        };
        assert!(reason.contains("`north nix gc`"), "{command}: {reason}");
    }
    assert_eq!(observed("north nix gc --dry-run"), "");
}

fn bash_in(cwd: &Path, command: &str) -> HookInput {
    let payload = json!({ "tool_name": "Bash", "session_id": "wisp-129", "cwd": cwd, "tool_input": { "command": command } });
    HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
}

#[test]
fn nix_ad_hoc_toolchain_in_a_dev_shell_worktree_is_denied_naming_the_dev_shell() {
    let root = tempfile::tempdir().unwrap();
    let worktree = root.path().join("code/wisp/worktrees/wisp-129");
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(worktree.join("flake.nix"), "{ outputs = { self, nixpkgs }: { devShells.x86_64-linux.default = {}; }; }\n").unwrap();
    let seen = tempfile::tempdir().unwrap();
    for command in ["nix shell nixpkgs#pkg-config -c cargo build", "nix run nixpkgs#hello", "nix-shell -p pkg-config --run 'cargo build'"] {
        let Verdict::Deny(reason) = decide(&bash_in(&worktree.join("src"), command), seen.path()) else {
            panic!("not denied: {command}");
        };
        assert!(reason.contains(&format!("{} has a dev shell", worktree.join("flake.nix").display())), "{command}: {reason}");
        assert!(reason.contains("`north dev-shell run -- COMMAND`") && classes(&reason).contains(&"N6".to_owned()), "{command}: {reason}");
    }
    assert!(!matches!(decide(&bash_in(&worktree, "nix shell .#north -c north --version"), seen.path()), Verdict::Deny(_)));
    std::fs::write(worktree.join("flake.nix"), "{ outputs = { self, nixpkgs }: { packages.x86_64-linux.default = {}; }; }\n").unwrap();
    assert!(!matches!(decide(&bash_in(&worktree, "nix shell nixpkgs#pkg-config -c cargo build"), seen.path()), Verdict::Deny(_)));
}
