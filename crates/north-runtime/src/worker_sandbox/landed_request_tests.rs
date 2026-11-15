//! A confined session's `north task landed` failed in its own domain with `attempt to write a readonly database` (gh-shim-1485 and confined-kernel-1485, 2026-11-14); the broker records the landing outside the domain for a caller in a fleet session's scope, as it writes a gate or withdraws a ticket.

use std::path::PathBuf;

use super::super::lane_grant::Host;
use super::record_request::ask;
use super::{OPEN_SCOPE, git};
use crate::foundation::shim::write_script;

const SESSION: &str = "w-landed-1485";

#[test]
fn a_confined_session_records_a_landing_without_writing_the_ledger() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let (home, fleet) = (root.join("home"), root.join("fleet"));
    let main = home.join("code/alpha/main");
    for directory in [main.clone(), fleet.join(SESSION)] {
        std::fs::create_dir_all(directory).unwrap();
    }
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(fleet.join(SESSION).join("host.scope"), "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n").unwrap();
    std::fs::write(fleet.join(SESSION).join("session.json"), serde_json::json!({ "name": SESSION, "cwd": main }).to_string()).unwrap();
    let recorded = root.join("recorded");
    let north = root.join("north");
    write_script(&north, &format!("echo \"$*\" >>{}\n", recorded.display()));
    let host = Host { fleet, home: home.clone(), code: vec![home.join("code")], environment: Vec::new(), cgroups: PathBuf::from("/sys/fs/cgroup") };
    let words = ["task", "landed", "north#1485", "0df34af262"].map(str::to_owned).to_vec();

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words, &main);
    let ran = std::fs::read_to_string(&recorded).unwrap_or_default();
    assert!(code == 0 && ran == "task landed north#1485 0df34af262\n", "the broker did not record the landing a confined session asked for: exit {code}, ran {ran:?}: {told}");
}
