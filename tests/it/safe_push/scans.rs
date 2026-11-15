use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_foundation::foundation::shim;

use super::{Fixture, Observed, git, write};

const SKIP: &str = concat!("it.sk", "ip(");
const MUTANT_MARKER: &str = concat!("MUT", "ANT:r1");

fn north(fixture: &Fixture, directory: &Path, arguments: &[&str]) -> Observed {
    let output = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .current_dir(directory)
        .args(arguments)
        .env("HOME", fixture.root.join("home"))
        .env("XDG_STATE_HOME", &fixture.state)
        .env("GIT_CONFIG_GLOBAL", fixture.scratch.path().join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Observed { status: output.status.code(), output: text }
}

fn publish_main(fixture: &Fixture) {
    fixture.git(&["push", "-q", "origin", "HEAD:main"]);
}

fn reset(fixture: &Fixture, to: &str) {
    fixture.git(&["reset", "-q", "--hard", to]);
}

fn commit_with(fixture: &Fixture, messages: &[&str]) {
    let mut arguments = vec!["commit", "-q"];
    for message in messages {
        arguments.extend(["-m", message]);
    }
    fixture.git(&arguments);
}

fn script(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    shim::write_script(path, "");
}

fn tools(fixture: &Fixture, names: &[&str]) {
    for name in names {
        script(&fixture.repo.join("dotfiles/bin").join(name));
    }
    fixture.git(&["add", "dotfiles/bin"]);
}

fn peer_north(fixture: &Fixture) -> PathBuf {
    let peer = fixture.root.join("home/code/north/main");
    std::fs::create_dir_all(&peer).unwrap();
    git(&peer, &["init", "-q", "-b", "main"]);
    script(&peer.join("bin/shared-tool"));
    git(&peer, &["add", "bin"]);
    git(&peer, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", "peer"]);
    git(&peer, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    peer
}

fn firn_origin(fixture: &Fixture) {
    let link = fixture.root.join("firn.git");
    std::os::unix::fs::symlink(&fixture.remote, &link).unwrap();
    for key in ["remote.origin.url", "remote.origin.pushurl"] {
        fixture.git(&["config", key, &link.to_string_lossy()]);
    }
}


#[test]
fn a_removed_tool_needs_a_replacement_in_the_peer_or_a_removes_tool_trailer_and_is_judged_against_the_merge_base() {
    let fixture = Fixture::new();
    firn_origin(&fixture);
    tools(&fixture, &["shared-tool", "lone-tool"]);
    fixture.git(&["commit", "-qm", "tools"]);
    publish_main(&fixture);
    let first = fixture.head();
    peer_north(&fixture);
    fixture.git(&["rm", "-q", "dotfiles/bin/lone-tool"]);
    fixture.git(&["commit", "-qm", "drop lone-tool"]);
    fixture.run(&["--dry-run"]).would_refuse("tool removal", &[]);
    reset(&fixture, &first);
    fixture.git(&["rm", "-q", "dotfiles/bin/shared-tool"]);
    fixture.git(&["commit", "-qm", "drop shared-tool"]);
    fixture.run(&["--dry-run"]).expect(true, &[]);

    reset(&fixture, &first);
    let names = ["agents", "threads", "safe-push", "retired-tool", "helper.test.sh"];
    tools(&fixture, &names);
    fixture.git(&["commit", "-qm", "all core tools"]);
    publish_main(&fixture);
    let base = fixture.head();
    let before = fixture.remote_state();
    for name in names {
        reset(&fixture, &base);
        fixture.git(&["rm", "-q", &format!("dotfiles/bin/{name}")]);
        commit_with(&fixture, &[&format!("retire {name}"), &format!("Removes-tool: {name}")]);
        let ran = fixture.run(&["--dry-run"]);
        match name {
            "safe-push" | "retired-tool" | "helper.test.sh" => ran.expect(true, &[]),
            _ => ran.would_refuse("tool removal", &[]),
        }
        assert_eq!(fixture.remote_state(), before, "a tool-removal dry run changed origin for {name}");
    }
    reset(&fixture, &base);
    fixture.git(&["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    fixture.git(&["commit", "-qm", "remove test helper without trailer"]);
    fixture.run(&["--dry-run"]).expect(true, &[]);

    reset(&fixture, &base);
    tools(&fixture, &["origin-only"]);
    fixture.git(&["commit", "-qm", "remote-only tool"]);
    let remote_head = fixture.head();
    reset(&fixture, &base);
    fixture.git(&["rm", "-q", "dotfiles/bin/helper.test.sh"]);
    fixture.git(&["commit", "-qm", "local test helper removal"]);
    let range = format!("{remote_head}..{}", fixture.head());
    north(&fixture, &fixture.repo, &["land", "scan", "tool-removal", &range]).expect(true, &[]);
}

fn test_integrity_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    write(&fixture.repo.join("retry.test.ts"), "expect(retry()).toBe(2)\n");
    fixture.git(&["add", "retry.test.ts"]);
    fixture.git(&["commit", "-qm", "existing retry test file"]);
    publish_main(&fixture);
    let base = fixture.head();
    (fixture, base)
}

#[test]
fn a_mutation_marker_a_skipped_test_and_a_lost_assertion_are_refused_unless_a_test_change_trailer_names_the_requirement() {
    let (fixture, base) = test_integrity_fixture();
    let before = fixture.remote_state();
    write(&fixture.repo.join("product.py"), &format!("answer = 41  # {MUTANT_MARKER}\n"));
    fixture.git(&["add", "product.py"]);
    commit_with(&fixture, &["product mutant", "Test-change: none"]);
    fixture.run(&["--dry-run"]).would_refuse("test integrity", &["adds a mutation marker"]);
    fixture.run(&["--dry-run", "--to", "other"]).expect(false, &["adds a mutation marker"]);
    assert_eq!(fixture.remote_state(), before, "a mutant refusal changed origin");
    for trailer in [None, Some("Test-change: the retry rule was removed")] {
        reset(&fixture, &base);
        write(&fixture.repo.join("retry.test.ts"), &format!("{SKIP}\"retries\", () => {{}})\n"));
        fixture.git(&["add", "retry.test.ts"]);
        let mut messages = vec!["skip retry test"];
        messages.extend(trailer);
        commit_with(&fixture, &messages);
        let ran = fixture.run(&["--dry-run"]);
        if trailer.is_some() { ran.expect(true, &[]) } else { ran.would_refuse("test integrity", &["adds a skip or focus"]) }
    }
    {
        reset(&fixture, &base);
        fixture.git(&["rm", "-q", "retry.test.ts"]);
        commit_with(&fixture, &["delete the retry test"]);
        fixture.run(&["--dry-run"]).expect(true, &[]);
    }
    for trailer in [None, Some("Test-change: the retry rule was removed")] {
        reset(&fixture, &base);
        write(&fixture.repo.join("retry.test.ts"), "retry()\n");
        fixture.git(&["add", "retry.test.ts"]);
        let mut messages = vec!["weaken the retry test"];
        messages.extend(trailer);
        commit_with(&fixture, &messages);
        let ran = fixture.run(&["--dry-run"]);
        if trailer.is_some() {
            ran.expect(true, &[])
        } else {
            ran.would_refuse("test integrity", &["removes 1 assertion and adds 0", "expect(retry()).toBe(2)"])
        }
    }
}

