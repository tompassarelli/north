//! A worker's `north land withdraw` failed in its own domain with `lifecycle store: attempt to write a readonly database` and a lead had to run it (north#1054, 2026-11-13); the broker runs it for the session whose scope the caller runs in, and only for a ticket that session queued.

use std::path::PathBuf;

use north_base::platform::land_queue;

use super::super::lane_grant::Host;
use super::record_request::ask;
use super::{OPEN_SCOPE, git};
use crate::foundation::shim::write_script;

const SESSION: &str = "w-withdraw-1054";
const ORIGIN: &str = "https://github.com/o/alpha.git";

#[test]
fn the_broker_withdraws_a_ticket_only_for_the_session_that_queued_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let (home, state, fleet) = (root.join("home"), root.join("state"), root.join("fleet"));
    let main = home.join("code/alpha/main");
    let queue = land_queue::directory(&state, ORIGIN);
    for directory in [main.clone(), fleet.join(SESSION), queue.join(land_queue::OUTBOX)] {
        std::fs::create_dir_all(directory).unwrap();
    }
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["remote", "add", "origin", ORIGIN]);
    std::fs::write(fleet.join(SESSION).join("host.scope"), "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n").unwrap();
    std::fs::write(fleet.join(SESSION).join("session.json"), serde_json::json!({ "name": SESSION, "cwd": main }).to_string()).unwrap();
    let ticket = |name: &str, session: &str| std::fs::write(queue.join(format!("{name}.json")), serde_json::json!({ "branch": name, "session": session }).to_string()).unwrap();
    ticket("mine", SESSION);
    ticket("theirs", "w-neighbour-1054");
    let withdrew = root.join("withdrew");
    let north = root.join("north");
    write_script(&north, &format!("echo \"$*\" >>{}\n", withdrew.display()));
    let host = Host { fleet, home: home.clone(), code: vec![home.join("code")], environment: vec![("XDG_STATE_HOME".to_owned(), state.to_string_lossy().into_owned())], cgroups: PathBuf::from("/sys/fs/cgroup") };
    let words = |ticket: &str| ["land", "withdraw", ticket, "--reason", "superseded"].map(str::to_owned).to_vec();

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("mine"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 0 && ran == "land withdraw mine --reason superseded\n", "the broker did not run the withdraw of the caller's own ticket: exit {code}, ran {ran:?}: {told}");

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("theirs"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 126 && told.contains("queued by w-neighbour-1054") && !ran.contains("theirs"), "the broker withdrew a ticket another session queued: exit {code}, ran {ran:?}: {told}");
}
