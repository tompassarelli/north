use std::fs;
use std::path::PathBuf;

use super::{Fixture, git, north_enqueue_with, stub_gh, written};

const PRIVATE_TERM: &str = "zebra-quokka-7731";

fn deny_list(fixture: &Fixture) -> PathBuf {
    let path = fixture.root.join("denylist.toml");
    fs::write(&path, format!("title = \"enqueue test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''{PRIVATE_TERM}'''\n")).unwrap();
    path
}

#[test]
fn enqueue_refuses_a_lane_carrying_a_deny_listed_value_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("identity.txt"), format!("owner: {PRIVATE_TERM}\n")).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "Add identity"]);
    let list = deny_list(&fixture);
    let path = stub_gh(&fixture, "echo PUBLIC");
    let (output, _) = north_enqueue_with(&fixture, &[("SAFE_PUSH_DENYLIST", &list), ("PATH", &path)]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("denylist: fake-private-term") && stdout.contains("for a PUBLIC repository"), "{stdout}{stderr}");
    assert!(stderr.contains("deny-list scan refused the landing") && stderr.contains("NOT queued"), "{stderr}");
    assert!(!stdout.contains(PRIVATE_TERM) && !stderr.contains(PRIVATE_TERM));
    assert!(!fixture.state.exists(), "a refused lane writes nothing to the state directory: {:?}", written(&fixture.state));
}

#[test]
fn enqueue_refuses_a_lane_a_secret_scan_flags_and_writes_no_ticket() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.repo, "a.txt", "Add a");
    let stub = fixture.root.join("stub-bin");
    fs::create_dir_all(&stub).unwrap();
    north_core::foundation::shim::write_executable(&stub.join("gitleaks"), "#!/bin/sh\nexit 1\n");
    let path = PathBuf::from(format!("{}:{}", stub.display(), std::env::var("PATH").unwrap_or_default()));
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("secret scan refused the landing"), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!fixture.state.exists(), "a refused lane writes no ticket");
}

