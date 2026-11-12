use super::behind::{Landing, Process, Seen, objects};
use super::page::conclude;
use super::samples;
use serde_json::json;
use std::cell::RefCell;

const OLD: &str = "aaaaaaaa1111111111111111111111111111aaaa";
const MIDDLE: &str = "bbbbbbbb2222222222222222222222222222bbbb";
const NEWEST: &str = "cccccccc3333333333333333333333333333cccc";

fn process(pid: u32, kind: &str, name: &str, runtime: Option<&str>) -> Process {
    Process { pid, kind: kind.to_owned(), process: name.to_owned(), runtime: runtime.map(str::to_owned) }
}

fn rank(sha: &str) -> u8 {
    [OLD, MIDDLE, NEWEST].iter().position(|known| *known == sha).map_or(0, |at| at as u8 + 1)
}

#[test]
fn runtime_lag_json_reports_seconds_behind_for_every_process_and_pages_once() {
    let landings = vec![Landing { at: 1_000, sha: MIDDLE.to_owned() }, Landing { at: 2_000, sha: NEWEST.to_owned() }];
    let seen = vec![
        Seen { host: "laptop".to_owned(), processes: Ok(vec![process(1, "land serve", "land serve all repositories", Some(NEWEST)), process(2, "session-host", "session-host worker-1", Some(MIDDLE)), process(3, "fleet watchdog", "fleet watchdog worker-1", None)]) },
        Seen { host: "nexus".to_owned(), processes: Ok(vec![process(4, "offload taker", "offload taker", Some(OLD))]) },
        Seen { host: "gone".to_owned(), processes: Err("no answer within 60s".to_owned()) },
    ];
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let found = objects(&seen, &landings, 2_500, holds);
    assert_eq!(
        found,
        vec![
            json!({"host": "laptop", "kind": "land serve", "process": "land serve all repositories", "runtime": NEWEST, "seconds": 0}),
            json!({"host": "laptop", "kind": "session-host", "process": "session-host worker-1", "runtime": MIDDLE, "seconds": 500}),
            json!({"host": "laptop", "kind": "fleet watchdog", "process": "fleet watchdog worker-1", "runtime": null, "seconds": 500}),
            json!({"host": "nexus", "kind": "offload taker", "process": "offload taker", "runtime": OLD, "seconds": 1_500}),
            json!({"host": "gone", "process": null, "runtime": null, "seconds": null, "unread": "no answer within 60s"}),
        ]
    );

    let state = tempfile::tempdir().unwrap();
    let pages = RefCell::new(Vec::new());
    let page = |text: &str| {
        pages.borrow_mut().push(text.to_owned());
        true
    };
    conclude(state.path(), &found, 2_490, |_| false);
    conclude(state.path(), &found, 2_500, page);
    conclude(state.path(), &found, 2_560, page);
    assert_eq!(pages.borrow().len(), 1, "a page that was delivered ends the paging for its episode, and one that was not is tried again");
    assert!(pages.borrow()[0].contains("offload taker on nexus") && pages.borrow()[0].contains("1500s"), "{:?}", pages.borrow());

    let recovered = objects(&seen[..1], &landings[..1], 1_100, holds);
    conclude(state.path(), &recovered, 1_100, page);
    conclude(state.path(), &found, 2_600, page);
    assert_eq!(pages.borrow().len(), 2, "lag that falls under 5 min ends the episode, so the next one pages again");
    let sampled = samples::read(state.path());
    assert_eq!(sampled.len(), 5);
    assert_eq!(samples::worst_in(&sampled, 2_000, 3_000), Some((1_500.0, 4)));
}
