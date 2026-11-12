use std::path::Path;

use super::*;

const BEFORE: &str = "pub const USAGE: &str = \"usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]\";\n";
const AFTER: &str = "pub const USAGE: &str = \"usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--app-token PROGRAM]\";\n";
const UNIT: &str = "      ExecStart = \"${landerRuntime}/tree/bin/north land serve --depth 4 ${serves}\";\n";
const UNIT_FIXED: &str = "      ExecStart = \"${landerRuntime}/tree/bin/north land serve ${serves}\";\n";

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(directory: &Path, files: &[(&str, &str)]) -> String {
    if !directory.join(".git").exists() {
        std::fs::create_dir_all(directory).unwrap();
        git_in(directory, &["init", "-q", "-b", "main"]);
    }
    for (path, text) in files {
        let file = directory.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        git_in(directory, &["add", path]);
    }
    git_in(directory, &["commit", "-qm", "change"]);
    git_in(directory, &["rev-parse", "HEAD"])
}

fn help(path: &[&str]) -> Option<String> {
    match path {
        ["land", "serve"] => Some(AFTER.to_owned()),
        ["land"] => Some("usage: north land serve|scan|checks".to_owned()),
        _ => None,
    }
}

fn run(north: &Path, base: &str, head: &str, home: &Path, state: &Path) -> (bool, String) {
    let mut out = Vec::new();
    let refused = scan(north, base, head, home, state, &mut help, &mut out);
    (refused, String::from_utf8(out).unwrap())
}

#[test]
fn a_removal_with_a_caller_in_another_checkout_fails_its_landing() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let (north, peer, wisp, state) = (root.path().join("north"), home.join("code/nixos-config/main"), home.join("code/wisp/main"), root.path().join("state"));
    let base = commit(&north, &[(REGISTRY, "[gate.peer-invocation]\nmode = \"shadow\"\n"), ("crates/north-landing/src/land/serve.rs", BEFORE)]);
    let head = commit(&north, &[("crates/north-landing/src/land/serve.rs", AFTER)]);
    commit(&peer, &[("modules/north-land/default.nix", UNIT)]);
    git_in(&peer, &["update-ref", PEER_MAIN, "HEAD"]);
    commit(&wisp, &[("scripts/serve.sh", "north land serve --depth 2\n")]);
    git_in(&wisp, &["update-ref", PEER_MAIN, "HEAD"]);

    let (refused, out) = run(&north, &base, &head, &home, &state);
    assert!(!refused && out.contains("would refuse (shadow)") && out.contains("modules/north-land/default.nix:1: runs `north land serve` with --depth") && out.contains(&format!("{}:1: runs `north land serve` with --depth", wisp.join("scripts/serve.sh").display())), "{out}");
    let log = std::fs::read_to_string(state.join("north/gates/peer-invocation.jsonl")).unwrap();
    assert!(log.contains("\"would_refuse\":true"), "{log}");

    let enforcing = commit(&north, &[(REGISTRY, "[gate.peer-invocation]\nmode = \"enforce\"\n")]);
    let (refused, out) = run(&north, &base, &enforcing, &home, &state);
    assert!(refused && out.contains("peer-invocation refused"), "{out}");

    commit(&peer, &[("modules/north-land/default.nix", UNIT_FIXED)]);
    git_in(&peer, &["update-ref", PEER_MAIN, "HEAD"]);
    commit(&wisp, &[("scripts/serve.sh", "north land serve\n")]);
    git_in(&wisp, &["update-ref", PEER_MAIN, "HEAD"]);
    let (refused, out) = run(&north, &base, &enforcing, &home, &state);
    assert!(!refused && out.is_empty(), "callers that no longer run --depth pass: {out}");
}
