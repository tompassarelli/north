use std::fs;
use std::process::Command;

use north_base::foundation::shim::write_executable;

// KEEP(bug): a landing's `bun install --frozen-lockfile` ran its packages' install scripts outside depbox, with the lander's home, logins and network in reach (south#93)
#[test]
fn dependency_install_runs_in_depbox() {
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("lane");
    fs::create_dir_all(checkout.join("node_modules")).unwrap();
    fs::write(checkout.join("package.json"), "{}\n").unwrap();
    fs::write(checkout.join("bun.lock"), "{}\n").unwrap();
    fs::write(checkout.join(".gitignore"), "node_modules\n").unwrap();
    for arguments in [&["init", "-q", "."][..], &["add", "."], &["-c", "user.name=n", "-c", "user.email=n@n", "commit", "-q", "-m", "lane"]] {
        assert!(Command::new("git").args(arguments).current_dir(&checkout).status().unwrap().success());
    }
    let north = scratch.path().join("north");
    let calls = scratch.path().join("calls");
    write_executable(&north, format!("#!/bin/sh\necho \"$*\" >>{}\n", calls.display()));
    super::sync_with(&checkout, &north).unwrap();
    assert_eq!(fs::read_to_string(&calls).unwrap(), "depbox fetch -- bun install --frozen-lockfile\n", "the install enters depbox's fetch sandbox");
}
