//! A confined session withdraws only a ticket it queued (north#1054), while a plain session or operator login outside every confined scope withdraws tickets without a fleet scope (north#1650).

use std::path::{Path, PathBuf};

use north_base::platform::land_queue;

use super::super::lane_grant::Host;
use super::record_request::ask;
use super::{OPEN_SCOPE, git};
use north_foundation::foundation::shim::write_script;

const SESSION: &str = "w-withdraw-1054";
const ORIGIN: &str = "https://github.com/o/alpha.git";
const PLAIN_CGROUP: &str = "0::/user.slice/user-1000.slice/session-1.scope\n";
const BROKER_CGROUP: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-worker-outside.service\n";
const LEASE_CGROUP: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent-batch.slice/agent-capacity-lease-1.scope\n";

struct Fleet {
    root: PathBuf,
    main: PathBuf,
    north: PathBuf,
    withdrew: PathBuf,
    host: Host,
}

fn words(ticket: &str) -> Vec<String> {
    ["land", "withdraw", ticket, "--reason", "superseded"].map(str::to_owned).to_vec()
}

fn fleet(root: &Path) -> Fleet {
    let root = root.to_path_buf();
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
    let ticket = |name: &str, session: Option<&str>| std::fs::write(queue.join(format!("{name}.json")), serde_json::json!({ "branch": name, "session": session }).to_string()).unwrap();
    ticket("mine", Some(SESSION));
    ticket("theirs", Some("w-neighbour-1054"));
    ticket("plain", None);
    let withdrew = root.join("withdrew");
    let north = root.join("north");
    write_script(&north, &format!("echo \"$*\" >>{}\n", withdrew.display()));
    let host = Host { fleet, home: home.clone(), code: vec![home.join("code")], environment: vec![("XDG_STATE_HOME".to_owned(), state.to_string_lossy().into_owned())], cgroups: PathBuf::from("/sys/fs/cgroup") };
    north_trusted_mirror::Mirror::open(&main).unwrap();
    Fleet { root, main, north, withdrew, host }
}

#[test]
fn the_broker_withdraws_a_ticket_only_for_the_session_that_queued_it() {
    let scratch = tempfile::tempdir().unwrap();
    let Fleet { main, north, withdrew, host, .. } = fleet(&scratch.path().canonicalize().unwrap());

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("mine"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 0 && ran == "land withdraw mine --reason superseded\n", "the broker did not run the withdraw of the caller's own ticket: exit {code}, ran {ran:?}: {told}");

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("theirs"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 126 && told.contains("queued by w-neighbour-1054") && !ran.contains("theirs"), "the broker withdrew a ticket another session queued: exit {code}, ran {ran:?}: {told}");

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("plain"), &main);
    assert!(code == 126 && told.contains("queued by no session"), "a confined worker withdrew an unattributed plain ticket: exit {code}: {told}");

    for escaped in [BROKER_CGROUP, LEASE_CGROUP] {
        let (code, told) = ask(&north, &host, escaped, &words("plain"), &main);
        assert_eq!(code, 126, "a broker or lease child withdrew outside its session scope ({escaped}): {told}");
    }

    let (code, told) = ask(&north, &host, PLAIN_CGROUP, &words("plain"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 0 && ran.contains("land withdraw plain --reason superseded\n"), "a plain session could not withdraw its ticket without a fleet scope: exit {code}, ran {ran:?}: {told}");

    let (code, told) = ask(&north, &host, PLAIN_CGROUP, &words("theirs"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 0 && ran.contains("land withdraw theirs --reason superseded\n"), "the operator login could not withdraw a queued ticket: exit {code}, ran {ran:?}: {told}");
}

#[test]
fn a_withdraw_names_its_queue_from_the_trusted_mirror() {
    let scratch = tempfile::tempdir().unwrap();
    let Fleet { root, main, north, withdrew, host, .. } = fleet(&scratch.path().canonicalize().unwrap());
    const ELSEWHERE: &str = "https://github.com/worker/own.git";
    git(&main, &["remote", "set-url", "--push", "origin", ELSEWHERE]);
    let own = land_queue::directory(&root.join("state"), ELSEWHERE);
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(own.join("theirs.json"), serde_json::json!({ "branch": "theirs", "session": SESSION }).to_string()).unwrap();

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("theirs"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 126 && told.contains("queued by w-neighbour-1054") && ran.is_empty(), "a repointed checkout push address moved the queue the broker reads: exit {code}, ran {ran:?}: {told}");

    let (code, told) = ask(&north, &host, OPEN_SCOPE, ["land", "withdraw", "theirs", "--repo=/tmp/not-a-project", "--reason", "superseded"].map(str::to_owned).as_ref(), &main);
    assert!(code == 126 && told.contains("in no project"), "an inline --repo=VALUE bypassed project resolution: exit {code}: {told}");

    let mirror = north_trusted_mirror::Mirror::directory(&main).unwrap();
    std::fs::remove_dir_all(&mirror).unwrap();
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("theirs"), &main);
    let ran = std::fs::read_to_string(&withdrew).unwrap_or_default();
    assert!(code == 126 && told.contains("keeps no trusted mirror") && ran.is_empty() && !mirror.exists(), "with no trusted mirror the broker took its queue from the checkout a worker writes: exit {code}, ran {ran:?}: {told}");
}
